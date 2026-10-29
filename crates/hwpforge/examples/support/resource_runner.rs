//! Decode-only resource observation with a counting system allocator.
use hwpforge_core::resource_experiment::{self as experiment, Experiment, Snapshot};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::Instant;

#[cfg(test)]
thread_local! {
    static THREAD_COUNTS: std::cell::Cell<(u64, i64)> = const { std::cell::Cell::new((0, 0)) };
}
struct Counting;
#[derive(Default)]
struct Counts {
    total: AtomicU64,
    live: AtomicU64,
    peak: AtomicU64,
}
static COUNTS: Counts =
    Counts { total: AtomicU64::new(0), live: AtomicU64::new(0), peak: AtomicU64::new(0) };
impl Counts {
    fn freed(&self, size: usize) {
        #[cfg(test)]
        if std::ptr::eq(self, &COUNTS) {
            let _ = THREAD_COUNTS.try_with(|c| {
                let (a, live) = c.get();
                c.set((a, live - size as i64));
            });
        }
        self.live.fetch_sub(size as u64, Relaxed);
    }
    fn start_call(&self) {
        self.peak.store(self.live.load(Relaxed), Relaxed);
    }
    fn allocated(&self, pointer: *mut u8, old: usize, size: usize) {
        if pointer.is_null() {
            return;
        }
        #[cfg(test)]
        if std::ptr::eq(self, &COUNTS) {
            let _ = THREAD_COUNTS.try_with(|c| {
                let (a, live) = c.get();
                c.set((a + size as u64, live - old as i64 + size as i64));
            });
        }
        self.total.fetch_add(size as u64, Relaxed);
        let previous = self
            .live
            .fetch_update(Relaxed, Relaxed, |live| Some(live - old as u64 + size as u64))
            .unwrap();
        let live = previous - old as u64 + size as u64;
        self.peak.fetch_max(live, Relaxed);
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        COUNTS.allocated(p, 0, layout.size());
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        COUNTS.allocated(p, 0, layout.size());
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(ptr, layout);
        }
        COUNTS.freed(layout.size());
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, size) };
        COUNTS.allocated(p, layout.size(), size);
        p
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
static START: OnceLock<Instant> = OnceLock::new();
struct Observation {
    a: AtomicU64,
    h: AtomicU64,
    live: AtomicU64,
    q: AtomicU64,
    elapsed: AtomicU64,
}
impl Observation {
    const fn new() -> Self {
        Self {
            a: AtomicU64::new(0),
            h: AtomicU64::new(0),
            live: AtomicU64::new(0),
            q: AtomicU64::new(0),
            elapsed: AtomicU64::new(0),
        }
    }
    fn capture(&self, s: Snapshot) {
        self.a.store(COUNTS.total.load(Relaxed), Relaxed);
        self.h.store(COUNTS.peak.load(Relaxed), Relaxed);
        self.live.store(COUNTS.live.load(Relaxed), Relaxed);
        self.q.store(s.q, Relaxed);
        self.elapsed.store(START.get().unwrap().elapsed().as_nanos() as u64, Relaxed);
    }
    fn row(
        &self,
        stage: &str,
        cfg: Experiment,
        case_id: &str,
        first: Option<u64>,
    ) -> serde_json::Value {
        serde_json::json!({
            "stage":stage, "case_id":case_id,
            "alloc_total_bytes":self.a.load(Relaxed), "live_peak_bytes":self.h.load(Relaxed),
            "live_bytes":self.live.load(Relaxed), "Q":self.q.load(Relaxed),
            "elapsed_ns":self.elapsed.load(Relaxed), "Q_at_first_reject":first,
            "timing":format!("{:?}",cfg.timing), "scope":format!("{:?}",cfg.scope), "budget":cfg.budget
        })
    }
}
static FIRST: Observation = Observation::new();
pub(crate) fn run(
    bytes: &[u8],
    entrypoint: &str,
    config: Experiment,
    case_id: &str,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    START.get_or_init(Instant::now);
    experiment::set_experiment(config);
    experiment::set_rejection_observer(|s| FIRST.capture(s));
    let applied = experiment::configuration().unwrap();
    let start = Observation::new();
    let complete = Observation::new();
    let dropped = Observation::new();
    COUNTS.start_call();
    start.capture(experiment::snapshot());
    let code = match entrypoint {
        "hwpx-decode" => {
            let result = hwpforge_smithy_hwpx::HwpxDecoder::decode(bytes);
            complete.capture(experiment::snapshot());
            let code = result.as_ref().err().map(|e| e.code());
            drop(result);
            dropped.capture(experiment::snapshot());
            code.map(|c| format!("{c:?}"))
        }
        "md-decode" => {
            let text = std::str::from_utf8(bytes)?;
            let result = hwpforge_smithy_md::MdDecoder::decode_with_default(text);
            complete.capture(experiment::snapshot());
            let code = result.as_ref().err().map(|e| e.code());
            drop(result);
            dropped.capture(experiment::snapshot());
            code.map(|c| format!("{c:?}"))
        }
        _ => return Err("invalid entrypoint".into()),
    };
    let first = experiment::snapshot().first_reject_q;
    let mut rows = vec![start.row("call_start", applied, case_id, None)];
    if first.is_some() {
        rows.push(FIRST.row("first_reject", applied, case_id, first));
    }
    rows.push(complete.row("call_complete", applied, case_id, first));
    rows.push(dropped.row("drop_complete", applied, case_id, first));
    for row in
        rows.iter_mut().filter(|r| r["stage"] == "call_complete" || r["stage"] == "drop_complete")
    {
        row["api_ok"] = code.is_none().into();
        row["error_code"] = serde_json::json!(code);
    }
    rows[0]["call_start_note"] = "MD UTF-8 validation is inside the measurement window.".into();
    experiment::clear_experiment();
    Ok(rows)
}

pub(crate) fn error_detail(
    bytes: &[u8],
    entrypoint: &str,
    config: Experiment,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    experiment::set_experiment(config);
    let detail = match entrypoint {
        "hwpx-decode" => {
            hwpforge_smithy_hwpx::HwpxDecoder::decode(bytes).err().map(|e| e.to_string())
        }
        "md-decode" => {
            hwpforge_smithy_md::MdDecoder::decode_with_default(std::str::from_utf8(bytes)?)
                .err()
                .map(|e| e.to_string())
        }
        _ => return Err("invalid entrypoint".into()),
    };
    experiment::clear_experiment();
    Ok(detail)
}

pub(crate) fn write_output(
    rows: &[serde_json::Value],
    output: &std::path::Path,
    case_id: &str,
    error_detail_enabled: bool,
    replay: impl FnOnce() -> Result<Option<String>, Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::File::create(output)?;
    serde_json::to_writer(&mut file, rows)?;
    file.sync_all()?;
    // Persist the measured call before a diagnostic replay can fail or be interrupted.
    if error_detail_enabled {
        let detail = replay()?;
        let mut path = output.as_os_str().to_os_string();
        path.push(".error-detail.json");
        let mut file = std::fs::File::create(path)?;
        serde_json::to_writer(
            &mut file,
            &serde_json::json!({
                "case_id": case_id, "error_detail": detail,
                "error_detail_source": "diagnostic replay after DROP"
            }),
        )?;
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 선택 플래그 없이 진단 재실행하기.
    #[test]
    fn output_without_detail_does_not_replay() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/no-detail-{}.json", std::process::id()));
        write_output(&[], &path, "test", false, || panic!("unexpected replay")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
        assert!(!std::path::PathBuf::from(format!("{}.error-detail.json", path.display())).exists());
        std::fs::remove_file(path).unwrap();
    }
    // 이것을 실패시키는 것: 관측 함수에서 JSON 직렬화로 할당하기, Q 또는 생존량 저장 생략.
    #[test]
    fn observation_does_not_allocate() {
        START.get_or_init(Instant::now);
        let observation = Observation::new();
        let before = THREAD_COUNTS.with(|c| c.get());
        let live_before = COUNTS.live.load(Relaxed);
        observation.capture(Snapshot { q: 13, first_reject_q: Some(13) });
        let live_after = COUNTS.live.load(Relaxed);
        let after = THREAD_COUNTS.with(|c| c.get());
        assert_eq!(before, after);
        assert!((live_before.min(live_after)..=live_before.max(live_after))
            .contains(&observation.live.load(Relaxed)));
        assert_eq!(observation.q.load(Relaxed), 13);
    }
    // 이것을 실패시키는 것: GlobalAlloc::dealloc의 freed 호출 삭제.
    #[test]
    fn box_drop_restores_live_bytes() {
        let before = THREAD_COUNTS.with(|c| c.get().1);
        let value = std::hint::black_box(Box::new([0u8; 64]));
        assert_eq!(THREAD_COUNTS.with(|c| c.get().1), before + 64);
        drop(value);
        assert_eq!(THREAD_COUNTS.with(|c| c.get().1), before);
    }
    // 이것을 실패시키는 것: realloc 누적량에 새 크기 대신 차이만 더하기.
    #[test]
    fn realloc_accounts_new_request_and_live_difference() {
        let counts = Counts::default();
        let pointer = std::ptr::dangling_mut::<u8>();
        counts.allocated(pointer, 0, 10);
        // 이것을 실패시키는 것: 실패한 요청을 성공량에 더하거나 이전 생존량에서 빼기.
        counts.allocated(std::ptr::null_mut(), 10, 100);
        counts.allocated(pointer, 10, 30);
        counts.allocated(pointer, 30, 5);
        assert_eq!(counts.total.load(Relaxed), 45);
        assert_eq!(counts.live.load(Relaxed), 5);
        assert_eq!(counts.peak.load(Relaxed), 30);
        // 이것을 실패시키는 것: dealloc 생존량 차감 삭제, 호출 시작 peak 재설정 삭제.
        counts.start_call();
        assert_eq!(counts.peak.load(Relaxed), 5);
        assert_eq!(counts.live.load(Relaxed), 5);
        counts.freed(5);
        assert_eq!(counts.live.load(Relaxed), 0);
    }
}
