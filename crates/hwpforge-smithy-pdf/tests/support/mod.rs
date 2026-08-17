//! W2b c3 — test-only PDF object/content-stream 파서.
//!
//! krilla 가 실제로 방출하는 PDF 바이트에서 이미지 XObject 의 배치
//! bbox 를 **byte grep 이 아니라 진짜 좌표 산술**로 추출한다 (§4 D3,
//! H5 disposition 수용). classic(비압축) xref 테이블 전제 — krilla 는
//! object stream/compressed xref 를 쓰지 않는다 (실측: `xref\n0 N\n...`
//! + `trailer` — 2026-08-17 probe).
//!
//! 알고리즘: `q`/`Q` CTM 스택 + 연속 `cm` 합성 + `/Name Do` 시
//! `/Resources /XObject` 사전으로 대상 오브젝트를 찾아 `/Subtype` 분기.
//! `/Image` 는 unit square 네 꼭짓점을 CTM 으로 변환해 bbox 산출,
//! `/Form` 은 자신의 `/Matrix`(없으면 항등) 를 CTM 에 합성하고 자신의
//! `/Resources`(없으면 호출측 상속)로 재귀한다.
//!
//! 범위 밖: object stream(compressed xref, PDF 1.5+), incremental
//! update(`/Prev` 체인), inline image(`BI`/`ID`/`EI`) — krilla 출력에
//! 등장하지 않아 미구현. 등장하면 파싱이 조용히 실패하지 않고 해당
//! 페이지의 이미지가 누락되므로 (경고 없이도) 테스트 assert 가 잡는다.

use std::collections::HashMap;
use std::io::Read as _;

// ── 공개 결과 타입 ───────────────────────────────────────────────

/// 한 이미지 XObject occurrence 의 페이지-공간 bbox (top-left 원점, pt).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageBbox {
    /// 페이지 `/Resources/XObject` 사전에서의 이름 (예: `"x0"`, `/` 제외).
    pub name: String,
    /// 좌상단 x (pt).
    pub x: f64,
    /// 좌상단 y (pt, top-left 기준 — `page_height - max_y`).
    pub y: f64,
    /// 표시 폭 (pt).
    pub width: f64,
    /// 표시 높이 (pt).
    pub height: f64,
    /// XObject 스트림의 `/Filter` (예: `Some("DCTDecode")` — JPEG passthrough 확인용).
    pub filter: Option<String>,
}

/// 한 쪽의 추출 결과.
#[derive(Debug, Clone)]
pub struct ExtractedPage {
    /// MediaBox 폭 (pt).
    pub width: f64,
    /// MediaBox 높이 (pt).
    pub height: f64,
    /// `/Image Do` 조우 순서(문서 그리기 순서) 그대로의 이미지 목록.
    pub images: Vec<ImageBbox>,
}

/// `tol` 이내로 근사 일치하는지.
pub fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// PDF 바이트에서 페이지별 이미지 bbox 를 추출한다.
///
/// # Panics
///
/// krilla 가 실제로 쓰지 않는 구조(object stream 등)를 만나 xref/trailer/
/// Root 를 못 찾으면 panic 한다 — 테스트 전용 도구라 조용한 빈 결과보다
/// 즉시 실패가 낫다.
pub fn extract_pages(pdf_bytes: &[u8]) -> Vec<ExtractedPage> {
    let doc = PdfDoc::load(pdf_bytes);
    let root_num = doc.trailer_root.expect("trailer /Root not found");
    let catalog = doc.objects.get(&root_num).expect("Root object missing");
    let pages_ref = catalog.get("Pages").expect("/Catalog missing /Pages");
    let pages_root = doc.resolve(pages_ref);
    let mut leaves = Vec::new();
    collect_page_leaves(&doc, pages_root, &mut leaves);

    leaves
        .into_iter()
        .map(|page| {
            let (x0, y0, x1, y1) = page_media_box(&doc, page);
            let width = (x1 - x0).abs();
            let height = (y1 - y0).abs();
            let resources = page_resources(&doc, page).unwrap_or(&Value::Null);
            let content = page_content_bytes(&doc, page);
            let mut raw_images: Vec<RawImageBbox> = Vec::new();
            extract_images_from_content(
                &doc,
                &content,
                resources,
                Matrix::IDENTITY,
                &mut raw_images,
                0,
            );
            let images = raw_images
                .into_iter()
                .map(|r| ImageBbox {
                    name: r.name,
                    x: r.min_x,
                    y: height - r.max_y,
                    width: r.max_x - r.min_x,
                    height: r.max_y - r.min_y,
                    filter: r.filter,
                })
                .collect();
            ExtractedPage { width, height, images }
        })
        .collect()
}

// ── 내부: PDF 값 모델 ────────────────────────────────────────────

// `Bool`/`Str` 의 내부 데이터는 절대 읽지 않는다 — 파서가 이 값들을
// 올바르게 "건너뛰기" 위해 존재를 인식해야 할 뿐이다(예: 사전 안
// `/Interpolate true`, trailer `/ID[(...)(...)]`). 의도적 dead payload.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Name(String),
    Str(Vec<u8>),
    Array(Vec<Value>),
    Dict(HashMap<String, Value>),
    Ref(u32),
    Stream(HashMap<String, Value>, Vec<u8>),
}

impl Value {
    fn as_dict(&self) -> Option<&HashMap<String, Value>> {
        match self {
            Value::Dict(d) | Value::Stream(d, _) => Some(d),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.as_dict().and_then(|d| d.get(key))
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn as_name(&self) -> Option<&str> {
        match self {
            Value::Name(n) => Some(n.as_str()),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }
}

struct PdfDoc {
    objects: HashMap<u32, Value>,
    trailer_root: Option<u32>,
}

impl PdfDoc {
    fn resolve<'a>(&'a self, v: &'a Value) -> &'a Value {
        match v {
            Value::Ref(n) => self.objects.get(n).unwrap_or(&Value::Null),
            other => other,
        }
    }

    /// classic xref 테이블 + trailer 로부터 오브젝트 테이블을 만든다.
    fn load(data: &[u8]) -> Self {
        let (offsets, xref_start, trailer_root) = parse_xref_and_trailer(data);
        let mut sorted: Vec<(u32, usize)> = offsets.into_iter().collect();
        sorted.sort_by_key(|&(_, off)| off);

        let mut objects = HashMap::new();
        for (idx, &(objnum, offset)) in sorted.iter().enumerate() {
            let upper = sorted.get(idx + 1).map(|&(_, o)| o).unwrap_or(xref_start);
            if offset >= data.len() || upper > data.len() || offset >= upper {
                continue;
            }
            let window = &data[offset..upper];
            let body_start = skip_object_header(window);
            let mut p = ValueParser::new(&window[body_start..]);
            let value = p.parse_value().unwrap_or(Value::Null);
            objects.insert(objnum, value);
        }
        Self { objects, trailer_root }
    }
}

/// `"N G obj"` 헤더를 건너뛰고 값이 시작하는 오프셋을 반환한다.
fn skip_object_header(window: &[u8]) -> usize {
    let mut pos = 0usize;
    skip_ws_and_comments(window, &mut pos);
    read_uint_token(window, &mut pos); // object number
    skip_ws_and_comments(window, &mut pos);
    read_uint_token(window, &mut pos); // generation
    skip_ws_and_comments(window, &mut pos);
    if window[pos..].starts_with(b"obj") {
        pos += 3;
    }
    pos
}

fn read_uint_token(data: &[u8], pos: &mut usize) -> Option<u32> {
    let start = *pos;
    while data.get(*pos).is_some_and(u8::is_ascii_digit) {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    std::str::from_utf8(&data[start..*pos]).ok()?.parse().ok()
}

fn is_pdf_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0C | 0x00)
}

fn is_pdf_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn skip_ws_and_comments(data: &[u8], pos: &mut usize) {
    loop {
        match data.get(*pos) {
            Some(&b) if is_pdf_ws(b) => *pos += 1,
            Some(&b'%') => {
                while let Some(&b) = data.get(*pos) {
                    *pos += 1;
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
}

// ── 내부: xref/trailer 파싱 ──────────────────────────────────────

/// `xref` 섹션(고전 표) + `trailer` 사전을 파싱한다.
///
/// 반환: (오브젝트 번호 → 파일 오프셋, xref 키워드 시작 오프셋(마지막
/// 오브젝트의 검색 상한 sentinel), trailer `/Root` 오브젝트 번호).
fn parse_xref_and_trailer(data: &[u8]) -> (HashMap<u32, usize>, usize, Option<u32>) {
    // `\n` 선행 필수 — 그냥 `b"xref"` 로 찾으면 `startxref` 키워드 꼬리의
    // "xref" 부분 문자열에 최종(rposition) 매치가 걸려 진짜 xref 표를
    // 건너뛰는 실측 버그가 났다 (`startxref` 는 파일 맨 끝, `xref` 표
    // 보다 뒤에 나온다 — 2026-08-18 e2e 디버그).
    let xref_pos = find_last(data, b"\nxref")
        .map(|p| p + 1)
        .or_else(|| if data.starts_with(b"xref") { Some(0) } else { None })
        .expect(
            "xref keyword not found (compressed-xref PDF unsupported by this test-only extractor)",
        );
    let mut pos = xref_pos + b"xref".len();
    let mut offsets = HashMap::new();

    loop {
        skip_ws_and_comments(data, &mut pos);
        if data[pos..].starts_with(b"trailer") {
            pos += b"trailer".len();
            break;
        }
        let Some(start) = read_uint_token(data, &mut pos) else { break };
        skip_ws_and_comments(data, &mut pos);
        let Some(count) = read_uint_token(data, &mut pos) else { break };
        for k in 0..count {
            skip_ws_and_comments(data, &mut pos);
            let Some(offset) = read_uint_token(data, &mut pos) else { break };
            skip_ws_and_comments(data, &mut pos);
            let _gen = read_uint_token(data, &mut pos);
            skip_ws_and_comments(data, &mut pos);
            let kind = data.get(pos).copied();
            if kind == Some(b'n') {
                offsets.insert(start + k, offset as usize);
            }
            pos += 1;
        }
    }

    skip_ws_and_comments(data, &mut pos);
    let mut p = ValueParser::new(&data[pos..]);
    let trailer = p.parse_value().unwrap_or(Value::Null);
    let root = trailer.get("Root").and_then(|v| match v {
        Value::Ref(n) => Some(*n),
        _ => None,
    });

    (offsets, xref_pos, root)
}

fn find_last(data: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || data.len() < needle.len() {
        return None;
    }
    data.windows(needle.len()).rposition(|w| w == needle)
}

// ── 내부: PDF 값 파서 (사전/배열/스트림/문자열/이름/숫자/참조) ────

struct ValueParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ValueParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        skip_ws_and_comments(self.data, &mut self.pos);
    }

    fn parse_value(&mut self) -> Option<Value> {
        self.skip_ws();
        match self.peek()? {
            b'<' if self.data.get(self.pos + 1) == Some(&b'<') => self.parse_dict_or_stream(),
            b'<' => Some(Value::Str(self.parse_hex_string())),
            b'(' => Some(Value::Str(self.parse_literal_string())),
            b'/' => Some(Value::Name(self.parse_name())),
            b'[' => Some(Value::Array(self.parse_array())),
            b'-' | b'+' | b'.' | b'0'..=b'9' => Some(self.parse_number_or_ref()),
            _ => {
                let kw = self.read_bare_keyword();
                match kw.as_str() {
                    "true" => Some(Value::Bool(true)),
                    "false" => Some(Value::Bool(false)),
                    "null" => Some(Value::Null),
                    "" => None,
                    _ => Some(Value::Null), // 알 수 없는 키워드 — 이 위치에선 발생하지 않는다.
                }
            }
        }
    }

    fn read_bare_keyword(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_pdf_ws(b) || is_pdf_delim(b) {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.data[start..self.pos]).into_owned()
    }

    fn parse_name(&mut self) -> String {
        self.pos += 1; // '/'
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if is_pdf_ws(b) || is_pdf_delim(b) {
                break;
            }
            if b == b'#'
                && self.data.get(self.pos + 1).is_some_and(u8::is_ascii_hexdigit)
                && self.data.get(self.pos + 2).is_some_and(u8::is_ascii_hexdigit)
            {
                let hex = std::str::from_utf8(&self.data[self.pos + 1..self.pos + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'#'));
                self.pos += 3;
            } else {
                out.push(b);
                self.pos += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn parse_number_or_ref(&mut self) -> Value {
        let (first, is_plain_uint) = self.read_number_raw();
        if is_plain_uint {
            let save = self.pos;
            self.skip_ws();
            let gen_start = self.pos;
            if self.peek().is_some_and(|b| b.is_ascii_digit()) {
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1;
                }
                let gen_end = self.pos;
                self.skip_ws();
                if self.peek() == Some(b'R')
                    && self.data.get(self.pos + 1).is_none_or(|&b| is_pdf_ws(b) || is_pdf_delim(b))
                {
                    self.pos += 1;
                    let _ = &self.data[gen_start..gen_end];
                    return Value::Ref(first as u32);
                }
            }
            self.pos = save;
        }
        Value::Number(first)
    }

    /// 숫자를 읽고 `(값, 부호/소수점 없는 순수 정수인지)` 를 반환한다.
    fn read_number_raw(&mut self) -> (f64, bool) {
        let start = self.pos;
        let mut plain_uint = true;
        if matches!(self.peek(), Some(b'-') | Some(b'+')) {
            plain_uint = false;
            self.pos += 1;
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else if b == b'.' {
                plain_uint = false;
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.data[start..self.pos]).unwrap_or("0");
        (text.parse().unwrap_or(0.0), plain_uint && !text.is_empty())
    }

    fn parse_literal_string(&mut self) -> Vec<u8> {
        self.pos += 1; // '('
        let mut depth = 1i32;
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'\\' => {
                    let Some(e) = self.peek() else { break };
                    match e {
                        b'n' => {
                            out.push(b'\n');
                            self.pos += 1;
                        }
                        b'r' => {
                            out.push(b'\r');
                            self.pos += 1;
                        }
                        b't' => {
                            out.push(b'\t');
                            self.pos += 1;
                        }
                        b'b' => {
                            out.push(0x08);
                            self.pos += 1;
                        }
                        b'f' => {
                            out.push(0x0C);
                            self.pos += 1;
                        }
                        b'(' | b')' | b'\\' => {
                            out.push(e);
                            self.pos += 1;
                        }
                        b'\r' => {
                            self.pos += 1;
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {
                            self.pos += 1;
                        }
                        b'0'..=b'7' => {
                            let mut val = 0u32;
                            let mut n = 0;
                            while n < 3 && matches!(self.peek(), Some(b'0'..=b'7')) {
                                val = val * 8 + u32::from(self.peek().unwrap() - b'0');
                                self.pos += 1;
                                n += 1;
                            }
                            out.push((val & 0xFF) as u8);
                        }
                        _ => {
                            out.push(e);
                            self.pos += 1;
                        }
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                _ => out.push(b),
            }
        }
        out
    }

    fn parse_hex_string(&mut self) -> Vec<u8> {
        self.pos += 1; // '<'
        let mut nibbles = Vec::new();
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if b.is_ascii_hexdigit() {
                nibbles.push(b);
            }
        }
        if nibbles.len() % 2 == 1 {
            nibbles.push(b'0');
        }
        nibbles
            .chunks(2)
            .map(|c| {
                let s = std::str::from_utf8(c).unwrap_or("00");
                u8::from_str_radix(s, 16).unwrap_or(0)
            })
            .collect()
    }

    fn parse_array(&mut self) -> Vec<Value> {
        self.pos += 1; // '['
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                None => break,
                _ => {
                    if let Some(v) = self.parse_value() {
                        out.push(v);
                    } else {
                        break;
                    }
                }
            }
        }
        out
    }

    fn parse_dict_or_stream(&mut self) -> Option<Value> {
        self.pos += 2; // '<<'
        let mut map = HashMap::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b'>') && self.data.get(self.pos + 1) == Some(&b'>') {
                self.pos += 2;
                break;
            }
            if self.peek() != Some(b'/') {
                break; // malformed — 방어적 종료
            }
            let key = self.parse_name();
            self.skip_ws();
            let value = self.parse_value()?;
            map.insert(key, value);
        }

        // `stream` 키워드가 뒤따르면 스트림 오브젝트.
        let save = self.pos;
        self.skip_ws();
        if self.data[self.pos..].starts_with(b"stream") {
            self.pos += b"stream".len();
            // 스펙: CRLF 또는 단독 LF (단독 CR 은 아님) 한 번만 스킵.
            if self.data.get(self.pos) == Some(&b'\r')
                && self.data.get(self.pos + 1) == Some(&b'\n')
            {
                self.pos += 2;
            } else if self.data.get(self.pos) == Some(&b'\n') {
                self.pos += 1;
            }
            let body_start = self.pos;
            // `/Length` 참조 해석 없이 `endstream` 리터럴로 경계를 찾는다
            // (오브젝트 검색창이 이미 다음 오브젝트 오프셋으로 상한 처리됨).
            let end = find_from(self.data, b"endstream", body_start).unwrap_or(self.data.len());
            let raw = self.data[body_start..end].to_vec();
            self.pos = end + b"endstream".len().min(self.data.len().saturating_sub(end));
            return Some(Value::Stream(map, raw));
        }
        self.pos = save;
        Some(Value::Dict(map))
    }
}

fn find_from(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= data.len() || needle.is_empty() {
        return None;
    }
    data[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

// ── 내부: 페이지 트리 워크 ────────────────────────────────────────

fn collect_page_leaves<'a>(doc: &'a PdfDoc, node: &'a Value, out: &mut Vec<&'a Value>) {
    if node.get("Type").and_then(Value::as_name) == Some("Pages") {
        if let Some(kids) = node.get("Kids").and_then(Value::as_array) {
            for kid in kids {
                collect_page_leaves(doc, doc.resolve(kid), out);
            }
        }
        return;
    }
    out.push(node);
}

fn page_media_box(doc: &PdfDoc, page: &Value) -> (f64, f64, f64, f64) {
    if let Some(mb) = page.get("MediaBox").and_then(Value::as_array) {
        let nums: Vec<f64> = mb.iter().map(|v| doc.resolve(v).as_number().unwrap_or(0.0)).collect();
        if nums.len() == 4 {
            return (nums[0], nums[1], nums[2], nums[3]);
        }
    }
    if let Some(parent) = page.get("Parent") {
        return page_media_box(doc, doc.resolve(parent));
    }
    (0.0, 0.0, 612.0, 792.0)
}

fn page_resources<'a>(doc: &'a PdfDoc, page: &'a Value) -> Option<&'a Value> {
    if let Some(r) = page.get("Resources") {
        return Some(doc.resolve(r));
    }
    let parent = page.get("Parent")?;
    page_resources(doc, doc.resolve(parent))
}

fn page_content_bytes(doc: &PdfDoc, page: &Value) -> Vec<u8> {
    let Some(contents_ref) = page.get("Contents") else { return Vec::new() };
    let contents = doc.resolve(contents_ref);
    let mut out = Vec::new();
    match contents {
        Value::Array(arr) => {
            for item in arr {
                let resolved = doc.resolve(item);
                out.extend(stream_bytes_decoded(resolved));
                out.push(b'\n');
            }
        }
        Value::Stream(..) => out.extend(stream_bytes_decoded(contents)),
        _ => {}
    }
    out
}

fn stream_bytes_decoded(v: &Value) -> Vec<u8> {
    let Value::Stream(dict, raw) = v else { return Vec::new() };
    match dict.get("Filter").and_then(Value::as_name) {
        Some("FlateDecode") => inflate(raw),
        _ => raw.clone(),
    }
}

fn inflate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dec = flate2::read::ZlibDecoder::new(data);
    if dec.read_to_end(&mut out).is_ok() {
        out
    } else {
        data.to_vec()
    }
}

// ── 내부: 2D 아핀 행렬 (PDF `cm` 합성) ────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// `m1` 을 먼저 적용하고 그 결과에 `m2` 를 적용하는 합성 행렬
    /// (`p' = p * m1 * m2`, row-vector 관례 — PDF `cm` 은 주어진 행렬을
    /// CTM 에 **선행 적용**하므로 `compose(given, ctm)`).
    fn compose(m1: Matrix, m2: Matrix) -> Matrix {
        Matrix {
            a: m1.a * m2.a + m1.b * m2.c,
            b: m1.a * m2.b + m1.b * m2.d,
            c: m1.c * m2.a + m1.d * m2.c,
            d: m1.c * m2.b + m1.d * m2.d,
            e: m1.e * m2.a + m1.f * m2.c + m2.e,
            f: m1.e * m2.b + m1.f * m2.d + m2.f,
        }
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

// ── 내부: 콘텐츠 스트림 렉서 (q/Q/cm/Do 만 의미 부여, 나머지는 스킵) ─

enum ContentTok {
    Num(f64),
    Name(String),
    Keyword(String),
}

struct ContentLexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ContentLexer<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        skip_ws_and_comments(self.data, &mut self.pos);
    }

    /// 문자열/사전/16진 문자열/배열을 통째로 건너뛴다 — 내부에 연산자와
    /// 같은 바이트열(`cm`, `Do` 등)이 우연히 들어 있어도 토큰으로 오인하지
    /// 않도록 opaque 값으로 취급한다 (TJ 배열의 문자열+커닝 숫자, BDC 의
    /// 속성 사전 등).
    fn skip_value(&mut self) {
        self.skip_ws();
        match self.peek() {
            Some(b'(') => self.skip_literal_string(),
            Some(b'<') if self.data.get(self.pos + 1) == Some(&b'<') => self.skip_dict(),
            Some(b'<') => self.skip_hex_string(),
            Some(b'[') => self.skip_array(),
            Some(b'/') => {
                self.pos += 1;
                while let Some(b) = self.peek() {
                    if is_pdf_ws(b) || is_pdf_delim(b) {
                        break;
                    }
                    self.pos += 1;
                }
            }
            Some(b'-') | Some(b'+') | Some(b'.') | Some(b'0'..=b'9') => {
                self.pos += 1;
                while let Some(b) = self.peek() {
                    if b.is_ascii_digit() || b == b'.' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            Some(_) => {
                while let Some(b) = self.peek() {
                    if is_pdf_ws(b) || is_pdf_delim(b) {
                        break;
                    }
                    self.pos += 1;
                }
            }
            None => {}
        }
    }

    fn skip_literal_string(&mut self) {
        self.pos += 1;
        let mut depth = 1i32;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'\\' => {
                    self.pos += 1; // 다음 한 바이트(이스케이프 대상)만 건너뜀 — 8진수는 길이만 다를 뿐 경계에 영향 없음(이미 소비한 첫 자리 밖 자리들은 일반 숫자로 재귀 소비됨).
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn skip_hex_string(&mut self) {
        self.pos += 1;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'>' {
                break;
            }
        }
    }

    fn skip_array(&mut self) {
        self.pos += 1;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return;
                }
                None => return,
                _ => self.skip_value(),
            }
        }
    }

    fn skip_dict(&mut self) {
        self.pos += 2;
        loop {
            self.skip_ws();
            if self.peek() == Some(b'>') && self.data.get(self.pos + 1) == Some(&b'>') {
                self.pos += 2;
                return;
            }
            if self.peek().is_none() {
                return;
            }
            self.skip_value(); // key
            self.skip_ws();
            self.skip_value(); // value
        }
    }

    fn next_token(&mut self) -> Option<ContentTok> {
        loop {
            self.skip_ws();
            let b = self.peek()?;
            match b {
                b'(' => {
                    self.skip_literal_string();
                    continue;
                }
                b'<' if self.data.get(self.pos + 1) == Some(&b'<') => {
                    self.skip_dict();
                    continue;
                }
                b'<' => {
                    self.skip_hex_string();
                    continue;
                }
                b'[' => {
                    self.skip_array();
                    continue;
                }
                b'/' => {
                    self.pos += 1;
                    let start = self.pos;
                    while let Some(c) = self.peek() {
                        if is_pdf_ws(c) || is_pdf_delim(c) {
                            break;
                        }
                        self.pos += 1;
                    }
                    return Some(ContentTok::Name(
                        String::from_utf8_lossy(&self.data[start..self.pos]).into_owned(),
                    ));
                }
                b'-' | b'+' | b'.' | b'0'..=b'9' => {
                    let start = self.pos;
                    self.pos += 1;
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() || c == b'.' {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    let text = std::str::from_utf8(&self.data[start..self.pos]).unwrap_or("0");
                    return Some(ContentTok::Num(text.parse().unwrap_or(0.0)));
                }
                b'{' | b'}' => {
                    // PostScript 계산 함수 블록(Type 4 함수) — 콘텐츠 스트림
                    // 자체엔 등장하지 않지만 방어적으로 한 바이트만 스킵.
                    self.pos += 1;
                    continue;
                }
                _ => {
                    let start = self.pos;
                    while let Some(c) = self.peek() {
                        if is_pdf_ws(c) || is_pdf_delim(c) {
                            break;
                        }
                        self.pos += 1;
                    }
                    if self.pos == start {
                        self.pos += 1; // 진행 보장
                        continue;
                    }
                    return Some(ContentTok::Keyword(
                        String::from_utf8_lossy(&self.data[start..self.pos]).into_owned(),
                    ));
                }
            }
        }
    }
}

struct RawImageBbox {
    name: String,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    filter: Option<String>,
}

fn extract_images_from_content(
    doc: &PdfDoc,
    content: &[u8],
    resources: &Value,
    initial_ctm: Matrix,
    out: &mut Vec<RawImageBbox>,
    depth: u32,
) {
    if depth > 8 {
        return; // Form XObject 병적 재귀 방지 (krilla 출력엔 등장하지 않음).
    }
    let mut lex = ContentLexer::new(content);
    let mut ctm_stack: Vec<Matrix> = vec![initial_ctm];
    let mut operands: Vec<f64> = Vec::new();
    let mut pending_name: Option<String> = None;

    while let Some(tok) = lex.next_token() {
        match tok {
            ContentTok::Num(n) => operands.push(n),
            ContentTok::Name(n) => pending_name = Some(n),
            ContentTok::Keyword(kw) => {
                match kw.as_str() {
                    "q" => {
                        let top = *ctm_stack.last().unwrap_or(&Matrix::IDENTITY);
                        ctm_stack.push(top);
                    }
                    "Q" => {
                        if ctm_stack.len() > 1 {
                            ctm_stack.pop();
                        }
                    }
                    "cm" if operands.len() >= 6 => {
                        let n = operands.len();
                        let given = Matrix {
                            a: operands[n - 6],
                            b: operands[n - 5],
                            c: operands[n - 4],
                            d: operands[n - 3],
                            e: operands[n - 2],
                            f: operands[n - 1],
                        };
                        if let Some(top) = ctm_stack.last_mut() {
                            *top = Matrix::compose(given, *top);
                        }
                    }
                    "Do" => {
                        if let Some(name) = pending_name.take() {
                            let ctm = *ctm_stack.last().unwrap_or(&Matrix::IDENTITY);
                            handle_do(doc, resources, &name, ctm, out, depth);
                        }
                    }
                    _ => {}
                }
                operands.clear();
                pending_name = None;
            }
        }
    }
}

fn handle_do(
    doc: &PdfDoc,
    resources: &Value,
    name: &str,
    ctm: Matrix,
    out: &mut Vec<RawImageBbox>,
    depth: u32,
) {
    let Some(xobjects) = resources.get("XObject").map(|v| doc.resolve(v)) else { return };
    let Some(xobj_ref) = xobjects.get(name) else { return };
    let xobj = doc.resolve(xobj_ref);
    let Value::Stream(dict, _) = xobj else { return };
    match dict.get("Subtype").and_then(Value::as_name) {
        Some("Image") => {
            let corners: [(f64, f64); 4] =
                [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| ctm.apply(x, y));
            let xs = corners.map(|c| c.0);
            let ys = corners.map(|c| c.1);
            let filter = dict.get("Filter").and_then(Value::as_name).map(str::to_string);
            out.push(RawImageBbox {
                name: name.to_string(),
                min_x: xs.iter().cloned().fold(f64::INFINITY, f64::min),
                max_x: xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                min_y: ys.iter().cloned().fold(f64::INFINITY, f64::min),
                max_y: ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                filter,
            });
        }
        Some("Form") => {
            let form_matrix = dict
                .get("Matrix")
                .and_then(Value::as_array)
                .map(|arr| {
                    let n: Vec<f64> =
                        arr.iter().map(|v| doc.resolve(v).as_number().unwrap_or(0.0)).collect();
                    if n.len() == 6 {
                        Matrix { a: n[0], b: n[1], c: n[2], d: n[3], e: n[4], f: n[5] }
                    } else {
                        Matrix::IDENTITY
                    }
                })
                .unwrap_or(Matrix::IDENTITY);
            let composed = Matrix::compose(form_matrix, ctm);
            let form_resources = dict.get("Resources").map(|v| doc.resolve(v)).unwrap_or(resources);
            let form_content = stream_bytes_decoded(xobj);
            extract_images_from_content(
                doc,
                &form_content,
                form_resources,
                composed,
                out,
                depth + 1,
            );
        }
        _ => {}
    }
}
