//! 청운시 문화유산 발굴연표 — wide table split across facing pages.
//!
//! This file contains completely fictional example data (~80 entries)
//! for the fictional city of 청운시. All site names, institution names,
//! district names, and publication titles are invented.
//! Creates interleaved left/right sections for paired page layout:
//!   - Odd pages (1,3,5,...):  연번, 시대, 연대, 유적명/위치, 출토유물
//!   - Even pages (2,4,6,...): 유적·유물의 내용, 조사기간, 조사기관, 출처, 비고, pdf
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwpx --example large_table
//!
//! Output:
//!   temp/large_table.hwpx

use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    Alignment, CharShapeIndex, Color, HwpUnit, LineSpacingType, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxFont, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

// ── Style indices ────────────────────────────────────────────────

const CS_NORMAL: usize = 0;
const CS_HEADER: usize = 1;
const CS_SMALL: usize = 2;
const PS_NORMAL: usize = 0;
const PS_CENTER: usize = 1;

// ── Column definitions for automatic page splitting ─────────────

/// Column metadata for the splitting/distribution algorithm.
struct ColumnDef {
    name: &'static str,
    min_width_mm: f64,
    para_shape_idx: usize,
    char_shape_idx: usize,
}

/// All 11 columns in reading order. The algorithm splits them across pages
/// based on which columns fit within the A4 portrait printable width (170mm).
const ALL_COLUMNS: [ColumnDef; 11] = [
    ColumnDef {
        name: "연번",
        min_width_mm: 8.0,
        para_shape_idx: PS_CENTER,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "시대",
        min_width_mm: 20.0,
        para_shape_idx: PS_CENTER,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "연대",
        min_width_mm: 24.0,
        para_shape_idx: PS_CENTER,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "유적명 / 위치",
        min_width_mm: 48.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "출토유물",
        min_width_mm: 70.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "유적·유물의 내용",
        min_width_mm: 60.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "조사기간",
        min_width_mm: 18.0,
        para_shape_idx: PS_CENTER,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "조사기관",
        min_width_mm: 25.0,
        para_shape_idx: PS_CENTER,
        char_shape_idx: CS_NORMAL,
    },
    ColumnDef {
        name: "출처",
        min_width_mm: 40.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_SMALL,
    },
    ColumnDef {
        name: "비고",
        min_width_mm: 15.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_SMALL,
    },
    ColumnDef {
        name: "pdf",
        min_width_mm: 12.0,
        para_shape_idx: PS_NORMAL,
        char_shape_idx: CS_SMALL,
    },
];

/// Font size used in data cells (pt).
const FONT_SIZE_PT: f64 = 8.0;
/// Line spacing percentage (130% = 1.3x font size).
const LINE_SPACING_PCT: u32 = 130;

// ── Helpers ──────────────────────────────────────────────────────

fn p(text: &str, cs: usize, ps: usize) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(cs))], ParaShapeIndex::new(ps))
}

/// Cell with multiple paragraphs (splits text on `\n`).
fn cell_lines(text: &str, width: HwpUnit, cs: usize, ps: usize) -> TableCell {
    let paras: Vec<Paragraph> = text.split('\n').map(|line| p(line, cs, ps)).collect();
    TableCell::new(paras, width)
}

/// Header cell with gray background.
fn header_cell(text: &str, width: HwpUnit) -> TableCell {
    let mut c = cell_lines(text, width, CS_HEADER, PS_CENTER);
    c.background = Some(Color::from_rgb(220, 220, 220));
    c
}

// ── Data ────────────────────────────────────────────────────────

#[derive(Clone)]
struct Row {
    num: &'static str,
    era: &'static str,
    era_span: u16,
    date: &'static str,
    site: &'static str,
    artifacts: &'static str,
    findings: &'static str,
    period: &'static str,
    institution: &'static str,
    source: &'static str,
    notes: &'static str,
    pdf: &'static str,
}

fn data() -> Vec<Row> {
    vec![
        // ── 구석기 (10) ──────────────────────────────────────────
        Row { num: "1", era: "구석기", era_span: 10, date: "전기구석기", site: "청운 운봉리 구석기유적\n운봉면 운봉리 12, 134번지 일원", artifacts: "찍개, 몸돌, 격지, 깨진 자갈돌, 부스러기 등 34점", findings: "석기 지표수습, 유물출토 문화층 미확인\n-표본조사 중 확인조사", period: "1998", institution: "청운문화재연구원", source: "청운문화재연구원, 2000,\n『청운 운봉리 구석기유적』", notes: "청동기시대 토기편 지표수습", pdf: "" },
        Row { num: "2", era: "구석기", era_span: 0, date: "중기구석기", site: "청운 하늘재 구석기유적\n하늘읍 하늘재리 산45-2번지 일원", artifacts: "흑요석, 석영제 긁개, 밀개, 홈날, 찌르개, 몸돌, 망치, 격지 등 2,184점", findings: "중기~후기구석기 문화층 3개소 확인", period: "2003~2004", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2006,\n『청운 하늘재 구석기유적』", notes: "", pdf: "" },
        Row { num: "3", era: "구석기", era_span: 0, date: "중기구석기", site: "청운 구름봉 유적(1지구)\n구름동 산22-1번지 일원", artifacts: "몸돌, 주먹도끼, 주먹찌르개, 주먹대패, 밀개, 긁개, 홈날, 격지 등 3,021점", findings: "중기구석기 문화층 4개소 확인", period: "2007~2009", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2011,\n『청운 구름봉 유적 1지구』", notes: "", pdf: "" },
        Row { num: "4", era: "구석기", era_span: 0, date: "중기구석기", site: "청운 구름봉 유적(2지구)\n구름동 산23-5번지 일원", artifacts: "찍개, 여러면석기, 긁개, 홈날, 복합석기 등 487점", findings: "후기구석기 문화층 2개소 확인", period: "2009~2011", institution: "달빛역사연구원", source: "달빛역사연구원, 2013,\n『청운 구름봉 유적 2지구』", notes: "", pdf: "" },
        Row { num: "5", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 별빛마을 구석기유적\n별내면 별빛리 89번지 일원", artifacts: "슴베찌르개, 좀돌날, 좀돌날몸돌, 흑요석제 새기개, 긁개 등 1,572점", findings: "후기구석기 문화층 3개소 확인\n-중기→후기 전환기 양상 확인", period: "2012~2013", institution: "청운문화재연구원", source: "청운문화재연구원, 2015,\n『청운 별빛마을 구석기유적』", notes: "", pdf: "" },
        Row { num: "6", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 솔밭들 구석기유적\n솔밭면 솔밭리 산78번지 일원", artifacts: "주먹도끼, 찍개, 뚜르개, 톱니날, 긁개, 밀개 등 891점", findings: "후기구석기 문화층 2개소 확인", period: "2014", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2016,\n『청운 솔밭들 구석기유적』", notes: "", pdf: "" },
        Row { num: "7", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 바람골 구석기유적\n바람골면 바람리 386-3번지 일원", artifacts: "흑요석제 화살촉, 슴베찌르개, 돌날 등 204점", findings: "후기구석기 초기~중기 2개 문화층 확인", period: "2005~2006", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2008,\n『청운 바람골 구석기유적』", notes: "", pdf: "" },
        Row { num: "8", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 수정터 구석기유적\n수정동 산56번지 일원", artifacts: "안팎날찍개, 뚜르개, 몸돌, 부스러기, 깨진 자갈돌 등 12점", findings: "시굴조사\n유물출토 문화층 미확인", period: "2016", institution: "달빛역사연구원", source: "달빛역사연구원, 2018,\n『청운 수정터 구석기유적』", notes: "", pdf: "" },
        Row { num: "9", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 은하벌 구석기유적\n은하면 은하리 일원", artifacts: "몸돌, 격지, 돌날몸돌, 돌날, 찍개, 여러면석기, 주먹대패 등 743점", findings: "문화층 2개소 확인", period: "2018~2019", institution: "청운문화재연구원", source: "청운문화재연구원, 2021,\n『청운 은하벌 구석기유적』", notes: "", pdf: "" },
        Row { num: "10", era: "구석기", era_span: 0, date: "후기구석기", site: "청운 새벽골 구석기유적\n새벽면 새벽리 115번지 일원", artifacts: "몸돌, 돌날, 밀개, 긁개, 슴베찌르개, 갈돌, 흑요석제 새기개 등 2,310점", findings: "문화층 3개소 확인", period: "2019~2021", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2023,\n『청운 새벽골 구석기유적』", notes: "", pdf: "" },

        // ── 신석기시대 (5) ────────────────────────────────────────
        Row { num: "11", era: "신석기시대", era_span: 5, date: "", site: "청운 노을리 신석기유적\n노을면 노을리 250-3번지 일원", artifacts: "빗살무늬토기편, 석촉 등 42점", findings: "수혈유구 1기, 노지 1기\n-선사시대 취락 존재가능성 확인", period: "2001", institution: "청운문화재연구원", source: "청운문화재연구원, 2003,\n『청운 노을리 신석기유적』", notes: "", pdf: "" },
        Row { num: "12", era: "신석기시대", era_span: 0, date: "", site: "청운 달빛고개 신석기유적\n달빛면 달빛리 303번지 일원", artifacts: "빗살무늬토기 구연부편, 갈돌, 고석 등 29점", findings: "수혈유구 2기\n-내부에 무시설식 노지 1기 확인", period: "2008~2009", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2011,\n『청운 달빛고개 신석기유적』", notes: "", pdf: "" },
        Row { num: "13", era: "신석기시대", era_span: 0, date: "", site: "청운 운봉리 신석기 주거지\n운봉면 운봉리 370-10번지 일원", artifacts: "즐문토기 발, 구연부편, 저부편, 긁개, 갈돌, 몸돌 등 56점", findings: "주거지 1기\n-평면 방형, 위석식 화덕시설 확인\n-대형에 속하는 주거지", period: "2013~2014", institution: "달빛역사연구원", source: "달빛역사연구원, 2016,\n『청운 운봉리 신석기 주거지』", notes: "", pdf: "" },
        Row { num: "14", era: "신석기시대", era_span: 0, date: "", site: "청운 수정동 선사유적\n수정동 산18번지 일원", artifacts: "즐문토기편, 결합식 낚시바늘, 뼈바늘 등 18점", findings: "패총 흔적 1개소, 수혈유구 1기\n-동물뼈 다수 출토", period: "2017", institution: "청운문화재연구원", source: "청운문화재연구원, 2019,\n『청운 수정동 선사유적』", notes: "지표조사 포함", pdf: "" },
        Row { num: "15", era: "신석기시대", era_span: 0, date: "", site: "청운 은하리 신석기유적\n은하면 은하리 188번지 일원", artifacts: "빗살무늬토기, 갈판, 갈돌, 석도편 등 33점", findings: "주거지 2기, 수혈 3기\n-취락 형성 초기 단계 양상 추정", period: "2020~2021", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2023,\n『청운 은하리 신석기유적』", notes: "", pdf: "" },

        // ── 청동기 (12) ───────────────────────────────────────────
        Row { num: "16", era: "청동기", era_span: 12, date: "전기", site: "청운 운봉리 청동기유적\n운봉면 운봉리, 달빛면 달빛리 일원", artifacts: "빗살무늬토기, 공렬토기, 무문토기,\n반달돌칼, 화살촉, 숫돌 등", findings: "주거지 5동", period: "2004~2005", institution: "청운문화재연구원", source: "청운문화재연구원, 2007,\n『청운 운봉리 청동기유적』", notes: "", pdf: "" },
        Row { num: "17", era: "청동기", era_span: 0, date: "전기", site: "청운 달빛리 마을유적\n달빛면 달빛리 303번지 일원", artifacts: "무문토기 호, 석촉, 반월형석도", findings: "주거지 3기\n-역삼동형 주거지 확인", period: "2006", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2008,\n『청운 달빛리 마을유적』", notes: "", pdf: "" },
        Row { num: "18", era: "청동기", era_span: 0, date: "전기", site: "청운 하늘재 청동기유적\n하늘읍 하늘재리 94번지 일원", artifacts: "무문토기편, 갈돌, 대석 등 21점", findings: "주거지 2기\n-세장방형 주거지\n-생활공간 외 작업공간 추정", period: "2009", institution: "달빛역사연구원", source: "달빛역사연구원, 2011,\n『청운 하늘재 청동기유적』", notes: "", pdf: "" },
        Row { num: "19", era: "청동기", era_span: 0, date: "전기", site: "청운 구름봉 고인돌\n구름동 산16-1번지 일원", artifacts: "", findings: "탁자식 지석묘 2기\n-재질 화강암, 남북방향\n*향토유적 제12호", period: "2002~2003", institution: "청운시문화원", source: "청운시문화원, 2004,\n『청운시 고인돌 조사보고서』", notes: "지표조사", pdf: "" },
        Row { num: "20", era: "청동기", era_span: 0, date: "전기", site: "청운 별빛리 고인돌\n별내면 별빛리 일원", artifacts: "", findings: "개석식 지석묘 3기", period: "2002~2003", institution: "청운시문화원", source: "청운시문화원, 2004,\n『청운시 고인돌 조사보고서』", notes: "지표조사", pdf: "" },
        Row { num: "21", era: "청동기", era_span: 0, date: "전기", site: "청운 솔밭리 고인돌\n솔밭면 솔밭리 일원", artifacts: "", findings: "탁자식 지석묘 1기 외 석재노출", period: "2002~2003", institution: "청운시문화원", source: "청운시문화원, 2004,\n『청운시 고인돌 조사보고서』", notes: "지표조사", pdf: "" },
        Row { num: "22", era: "청동기", era_span: 0, date: "전기", site: "청운 바람리 고인돌\n바람골면 바람리 8번지 일원", artifacts: "", findings: "탁자식 지석묘 2기\n*향토유적 제18호", period: "2002~2003", institution: "청운시문화원", source: "청운시문화원, 2004,\n『청운시 고인돌 조사보고서』", notes: "지표조사", pdf: "" },
        Row { num: "23", era: "청동기", era_span: 0, date: "전기", site: "청운 노을리 고인돌군\n노을면 노을리 251번지 일원", artifacts: "", findings: "탁자식 지석묘 1기\n*향토유적 제3호", period: "2002~2003", institution: "청운시문화원", source: "청운시문화원, 2004,\n『청운시 고인돌 조사보고서』", notes: "지표조사", pdf: "" },
        Row { num: "24", era: "청동기", era_span: 0, date: "전기", site: "청운 수정동 청동기유적\n수정동 434-4번지 일원", artifacts: "무문토기 4점, 석촉 2점", findings: "고인돌 3기 확인\n탁자식, 개석식 확인\n-유아묘 존재 가능성 추정", period: "2010~2011", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2013,\n『청운 수정동 청동기유적』", notes: "", pdf: "" },
        Row { num: "25", era: "청동기", era_span: 0, date: "중기", site: "청운 은하벌 청동기 취락\n은하면 은하리 일원", artifacts: "구순각목, 이중구연단사선문, 무문토기, 반월형석도, 석촉", findings: "주거지 4기, 수혈유구 5기\n역삼동식 주거지 확인", period: "2015~2016", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2018,\n『청운 은하벌 청동기 취락유적』", notes: "", pdf: "" },
        Row { num: "26", era: "청동기", era_span: 0, date: "중기", site: "청운 새벽리 유적\n새벽면 새벽리, 노을리 일원", artifacts: "신석기시대 토기편, 공열문토기, 무문토기, 반월형석도, 석촉, 석착, 지석 등", findings: "주거지 2기, 수혈 4기\n-북한강유형과 유사\n-임시거처로 추정", period: "2018~2019", institution: "달빛역사연구원", source: "달빛역사연구원, 2021,\n『청운 새벽리 유적』", notes: "", pdf: "" },
        Row { num: "27", era: "청동기", era_span: 0, date: "후기", site: "청운 은하면 늘터 유적\n은하면 은하리 133-1번지 일원", artifacts: "주상편인석부, 반월형석도, 무문토기 호편 등 28점", findings: "주거지 2기, 소성유구 1기\n-송국리형 주거지 확인", period: "2021~2022", institution: "청운문화재연구원", source: "청운문화재연구원, 2024,\n『청운 은하면 늘터 유적』", notes: "", pdf: "" },

        // ── 초기철기~원삼국 (10) ──────────────────────────────────
        Row { num: "28", era: "초기철기~원삼국", era_span: 10, date: "1~2세기", site: "청운 달빛리 유적\n달빛면 달빛리 374-3번지 일원", artifacts: "민무늬토기, 타날문토기편, 돌끌, 갈판, 토우, 대롱옥 등", findings: "주거지 2기\n-평면형태 장방형, 내부시설 노지, 주혈", period: "1997", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1999,\n『청운 달빛리 유적』", notes: "", pdf: "" },
        Row { num: "29", era: "초기철기~원삼국", era_span: 0, date: "1~2세기", site: "청운 솔밭리 취락유적\n솔밭면 솔밭리 1014-5번지 일원", artifacts: "무문토기편, 경질무문토기편, 타날문토기편, 화분형토기편, 토제방추차, 석제그물추, 석제화살촉, 숫돌, 철경동촉 등 188점", findings: "주거지 8기, 불탄자리 2기, 구상유구 4기\n-철자형, 장방형 주거지 확인\n-돌 위에 기둥을 세운 양상 확인", period: "2000~2001", institution: "청운문화재연구원", source: "청운문화재연구원, 2003,\n『청운 솔밭리 취락유적』", notes: "", pdf: "" },
        Row { num: "30", era: "초기철기~원삼국", era_span: 0, date: "1~2세기", site: "청운 구름동 421번지 유적\n구름동 421번지 일원", artifacts: "경질무문토기 구연부편, 기저부편", findings: "주거지 2기, 지상식건물지 1기\n-평면형태 장방형\n-내부시설 노지, 주혈", period: "2006", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2008,\n『청운 구름동 421번지 유적』", notes: "", pdf: "" },
        Row { num: "31", era: "초기철기~원삼국", era_span: 0, date: "1~2세기", site: "청운 바람리 모래내유적\n바람골면 바람리 35번지 일원", artifacts: "중도식무문토기, 타날문토기, 시루, 연질토기, 토제방추차, 원형토제품, 지석, 구슬 등 132점", findings: "주거지 22기, 수혈유구 10기\n-구릉 정상부에 밀집분포\n-凸자형 주거지 주류\n-외줄구들, 부뚜막, 노지, 주공 확인", period: "2009", institution: "달빛역사연구원", source: "달빛역사연구원, 2011,\n『청운 바람리 모래내유적』", notes: "", pdf: "" },
        Row { num: "32", era: "초기철기~원삼국", era_span: 0, date: "1~2세기", site: "청운 노을리 유적(1지점)\n노을면 노을리 291-2번지", artifacts: "경질무문토기 외반구연 옹, 호 등", findings: "주거지 3기, 수혈유구 2기, 주혈군 2기\n-주거지 평면형태 말각방형\n-'ㅡ','ㄱ'자형 부뚜막, 주혈 확인", period: "2012", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2014,\n『청운 노을리 유적 1·2지점』", notes: "", pdf: "" },
        Row { num: "33", era: "초기철기~원삼국", era_span: 0, date: "2~3세기", site: "청운 별빛리 선사유적\n별내면 별빛리 269 외", artifacts: "즐문토기편, 중도식외반구연호, 심발형토기, 승석문단경호, 호형토기, 무문토기시루편, 대옹, 토제방추차, 지석, 철모, 철도자 등", findings: "원삼국시대 주거지 6기, 미상수혈유구 2기\n-포천지역 처음 이루어진 발굴조사와 유사\n-중소형 철자형주거지 확인\n-외줄구들의 부뚜막 확인", period: "1994", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1996,\n『청운 별빛리 선사유적』", notes: "", pdf: "" },
        Row { num: "34", era: "초기철기~원삼국", era_span: 0, date: "2~3세기", site: "청운 노을동 마을유적\n노을면 노을리 일원", artifacts: "즐문토기편, 단경호, 타날문토기편, 심발형토기편, 회청색경질토기편, 대부완, 동이, 고배, 외반구연옹, 기대, 주조철부편, 철편, 철촉, 철겸 등 621점", findings: "원삼국시대 주거지 2기, 한성기 주거지 3기, 소형유구 8기, 구상유구 1기\n-성동리산성과 연관된 유적과 유사 양상\n-한성기 유구의 분포양상 구분", period: "2001", institution: "청운문화재연구원", source: "청운문화재연구원, 2003,\n『청운 노을동 마을유적』", notes: "", pdf: "" },
        Row { num: "35", era: "초기철기~원삼국", era_span: 0, date: "2~3세기", site: "청운 구름동 취락유적Ⅰ\n구름동 251-2번지 일원", artifacts: "격자·사격자문·무문 암키와, 대옹, 대호, 직구호, 동이, 원저단경호, 장란형토기, 심발형토기, 원통형기대, 철도자, 철정 등", findings: "주거지 3기, 소형유구 5기, 구상유구 3기, 굴립주건물지 1기\n-대형 주거지\n-위계가 높은 집단으로 추정\n-자체 철기 생산 가능성 추정", period: "2003~2004", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2006,\n『청운 구름동 취락유적Ⅰ』", notes: "", pdf: "" },
        Row { num: "36", era: "초기철기~원삼국", era_span: 0, date: "2~3세기", site: "청운 달빛리 취락유적Ⅱ\n달빛면 달빛리 250-3번지 일원", artifacts: "대옹, 원통형기대편, 소형기대편, 원저단경호, 타날문토기편, 경질무문토기편, 철촉편, 철도자편, 지석 등", findings: "주거지 35~38기 존재 추정, 소형유구 70기 내외, 구상유구 3기\n-시굴조사로 유적 범위 확인\n-백제시대 주거지 비율 높음", period: "2005", institution: "달빛역사연구원", source: "달빛역사연구원, 2007,\n『청운 달빛리 취락유적Ⅱ』", notes: "", pdf: "" },
        Row { num: "37", era: "초기철기~원삼국", era_span: 0, date: "2~3세기", site: "청운 바람리 용수골 유적\n바람골면 바람리 552번지 일원", artifacts: "경질무문토기, 타날문토기, 이형토기, 회색무문양토기, 토제품, 석제품, 마노구슬, 철촉, 환두소도 등", findings: "주거지 28기, 수혈유구 46기, 소성유구 4기, 고상가옥 5기, 목책 3기\n-凸자형 주거지 주류\n-낙랑계 기술영향 확인", period: "2014~2016", institution: "청운문화재연구원", source: "청운문화재연구원, 2018,\n『청운 바람리 용수골 유적』", notes: "", pdf: "" },

        // ── 삼국~통일신라 (10) ────────────────────────────────────
        Row { num: "38", era: "삼국~통일신라", era_span: 10, date: "", site: "청운 고소산성\n달빛면 달빛리 산2-1번지 일원", artifacts: "토기편, 기와편 등", findings: "테뫼식 석축산성\n둘레 512m\n토기편으로 볼 때 삼국시대로 추정", period: "1997", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1998,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "39", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 소봉산성\n솔밭면 솔밭리 할미산", artifacts: "불명철기편, 토기편 등", findings: "테뫼식 석축산성\n둘레 94m\n막돌허튼층쌓기\n소규모 보루로 교통로 차단 목적 추정", period: "1997", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1998,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "40", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 바람산성\n바람골면 바람리 산225-6 일원", artifacts: "고배편, 인화문토기편, 토기편, 기와편 등", findings: "원형의 석축산성\n-해발 215m, 둘레 280m\n-서쪽성벽 일부 잔존 확인\n-단기적 방어를 위한 성 추정", period: "1998", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1999,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "41", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 달빛산성\n달빛면 달빛리 산61-1번지 일원", artifacts: "개배, 고배, 완, 뚜껑, 합, 심발형토기, 타날문토기편, 중국제 시유도기편 등", findings: "동벽일대 석축성벽, 수혈유구\n-2단의 석축부, 기초부만 잔존\n-토축성벽을 석축성벽으로 개축\n-성 내 정상부 생활시설 잔존 가능성", period: "2007~2008", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2010,\n『청운 달빛산성Ⅰ』", notes: "", pdf: "" },
        Row { num: "42", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 노을리 유적(1·2지점)\n노을면 노을리 291-2번지, 산23-1번지 일원", artifacts: "단경호, 완, 호, 파수부호, 파수부완, 대부완, 옹, 병, 고배, 부가구연대부장경호, 암키와편, 철제테두리 등", findings: "수혈주거지 7기, 수혈 9개, 주혈군 3개소, 석실묘 5기, 석곽묘 3기\n-주거지 평면형태 방형, 장방형, 凸자형\n-석실묘 평면형태 대부분 장방형", period: "2014", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2016,\n『청운 노을리 유적 1·2지점』", notes: "", pdf: "" },
        Row { num: "43", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 구름봉 취락유적Ⅱ\n구름동 250-3번지 일원", artifacts: "수키와편, 암키와편, 경질무문토기, 장란형토기, 심발형토기, 단경호, 회색무문양토기 호, 대옹, 시루, 도자편, 주조괭이, 철정, 숫돌 등", findings: "주거지 18기, 지상식건물지 1기, 수혈 60기, 구상유구 5기\n-凸, 呂자형 출입구의 오각형 주거지 주류\n-내부시설 'ㄱ','ㅡ'자형 부뚜막\n-저장용 수혈 다수", period: "2010", institution: "청운문화재연구원", source: "청운문화재연구원, 2012,\n『청운 구름봉 취락유적Ⅱ』", notes: "", pdf: "" },
        Row { num: "44", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 수정동 419-32번지 유적\n수정동 419-32번지 일원", artifacts: "적갈색 연질토기 완, 고배편", findings: "수혈식석곽묘 2기\n-비교적 대형에 속하는 것\n-내부시설 시상", period: "2016", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2018,\n『청운 수정동 419-32번지 유적』", notes: "", pdf: "" },
        Row { num: "45", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 은하리 기와가마\n은하면 은하리 산15-31번지 일원", artifacts: "암키와편, 수키와편, 수막새편", findings: "기와가마 2기, 용도미상가마 1기", period: "2018~2019", institution: "달빛역사연구원", source: "달빛역사연구원, 2021,\n『청운 은하리 기와가마유적』", notes: "", pdf: "" },
        Row { num: "46", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 새벽리 복합유적\n새벽면 새벽리 373-2번지 일원", artifacts: "기와편, 토기 호, 대호, 도기뚜껑, 완, 매병편, 청자화형접시, 분청접시, 백자발, 철솥 편 등", findings: "주거지 3기, 삼가마 1기, 구들, 소성유구 2기, 석렬 2기\n-주거지 평면형태 장방형\n-삼가마 일체형 1기", period: "2019", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2021,\n『청운 새벽리 복합유적』", notes: "", pdf: "" },
        Row { num: "47", era: "삼국~통일신라", era_span: 0, date: "", site: "청운 반월성\n하늘읍 하늘리 산 5-1번지 일원", artifacts: "명문기와, 수막새, 귀면와, 무문·격자문·사격자문 평기와, 장동호, 호, 단경호, 완, 대부발, 벼루편, 고배, 철제도끼, 철겸, 철정, 방추차, 어망추 등", findings: "장대지, 건물지 2기, 성벽 단면, 북문지 조사\n*도기념물\n-장대지, 건물지 대지조성 확인\n-체성벽 수직에 가깝게 품자형 축조\n-북문지 평거식 확인", period: "1999", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2000,\n『청운 반월성 1차 발굴조사 보고서』", notes: "", pdf: "" },

        // ── 고려 (12) ─────────────────────────────────────────────
        Row { num: "48", era: "고려", era_span: 12, date: "", site: "청운 보가산성\n바람골면 바람리 산251-1번지 일원", artifacts: "기와편, 청자편", findings: "포곡식 석축산성\n-전체 둘레 3.8㎞\n-성문지, 수구부, 추정건물지 확인\n-고려시대에 축조 추정", period: "1996", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1997,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "49", era: "고려", era_span: 0, date: "", site: "청운 운악산성\n달빛면 산202번지 일원", artifacts: "어골문, 격자문 기와편, 회청색 경질토기", findings: "편축과 협축을 혼용한 석축산성\n-해발 870m\n-내부에서 추정문지, 탄요 흔적 확인\n-입보형 성곽 추정", period: "1998", institution: "청운대학교 박물관", source: "청운대학교 박물관, 1999,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "50", era: "고려", era_span: 0, date: "", site: "청운 노을리 태봉\n노을면 노을리 산28-2 일원", artifacts: "", findings: "왕녀 아기의 재를 묻은 곳\n-태항 도굴, 석대와 개석만 잔존", period: "2000~2001", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2002,\n『청운시 문화유적분포지도』", notes: "", pdf: "" },
        Row { num: "51", era: "고려", era_span: 0, date: "1185년", site: "청운향교\n하늘읍 하늘리 176", artifacts: "", findings: "외삼문, 내삼문, 명륜당, 대성전 등\n*문화유산자료\n-1185년 창건, 1592년 소실\n-1598년 중건, 1975년 중수", period: "2000~2001", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2002,\n『청운시 문화유적분포지도』", notes: "지표조사", pdf: "" },
        Row { num: "52", era: "고려", era_span: 0, date: "", site: "청운 솔밭리 유적\n솔밭면 솔밭리 산70-11 일원", artifacts: "토기편, 도기 호, 백자종지, 주조괭이, 물미, 쇠삽날, 철검, 철촉, 철정 등", findings: "수혈건물지 2기, 수혈유구 1기\n-2열의 구들열, 아궁이, 배연시설 확인\n-철기유물 다량 확인\n조선시대 화전민 거주 추정", period: "2005", institution: "청운문화재연구원", source: "청운문화재연구원, 2007,\n『청운 솔밭리 유적』", notes: "", pdf: "" },
        Row { num: "53", era: "고려", era_span: 0, date: "", site: "청운 달빛리 마산유적\n달빛면 달빛리 303번지 일원", artifacts: "분청자, 백자, 동이, 도기편, 기와편 등", findings: "고려~조선시대 건물지 1기,\n조선시대 주거지 1기, 수혈유구 12기\n유물지 1기", period: "2008~2009", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2011,\n『청운 달빛리 마산유적』", notes: "", pdf: "" },
        Row { num: "54", era: "고려", era_span: 0, date: "", site: "청운 구름봉 유적\n구름동, 달빛면 달빛리 일원", artifacts: "청자접시 및 완, 백자접시 및 병,\n도기 완, 시루, 평기와, 철부, 청동숟가락 등", findings: "조선시대 건물지 2동, 흑탄요 1기, 집석유구 1기, 수혈유구 3기, 토광묘 3기, 회곽묘 1기\n-고려 말~조선 후기 출토유물", period: "2009~2010", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2012,\n『청운 구름봉 유적』", notes: "", pdf: "" },
        Row { num: "55", era: "고려", era_span: 0, date: "", site: "청운 바람리 유적\n바람골면 바람리 산15-31번지 일원", artifacts: "암키와편, 수키와편", findings: "건물지 2기, 석렬 1기, 집석 1기, 토광묘 5기, 회곽묘 6기, 구상유구 2기, 수혈 5기", period: "2011~2012", institution: "달빛역사연구원", source: "달빛역사연구원, 2014,\n『청운 바람리 유적』", notes: "", pdf: "" },
        Row { num: "56", era: "고려", era_span: 0, date: "", site: "청운 은하리 청자가마\n은하면 은하리 산82번지 일원", artifacts: "분청자기 호편, 청자발·접시·호·병·잔·매병, 초벌 저부편, 흑유주자, 도침, 시침, 지석, 동전 등", findings: "고려시대 청자가마 1기, 폐기장 2기, 석곽묘 1기, 수혈유구 2기, 조선시대 숯가마 3기, 토광묘 6기, 회곽묘 2기", period: "2015~2016", institution: "청운문화재연구원", source: "청운문화재연구원, 2018,\n『청운 은하리 청자가마유적』", notes: "", pdf: "" },
        Row { num: "57", era: "고려", era_span: 0, date: "", site: "청운 새벽리 산성지\n새벽면 새벽리 산110번지 일원", artifacts: "기와편, 청자편, 소형토기 등", findings: "테뫼식 석축산성\n둘레 320m\n-배수로, 문지 추정지 확인", period: "2017", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2019,\n『청운 새벽리 산성지 시굴조사 보고서』", notes: "지표조사 포함", pdf: "" },
        Row { num: "58", era: "고려", era_span: 0, date: "", site: "청운 노을동 고분군\n노을면 노을리 산45번지 일원", artifacts: "청자 접시, 청자 대접, 철기편, 동경편 등", findings: "석실묘 3기, 석곽묘 5기\n-고려 중기~후기 조성으로 추정\n-내부 시상대 확인", period: "2019~2020", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2022,\n『청운 노을동 고분군』", notes: "", pdf: "" },
        Row { num: "59", era: "고려", era_span: 0, date: "", site: "청운 별빛마을 건물지\n별내면 별빛리 668-1번지 일원", artifacts: "기와편, 청자편, 소형호 등", findings: "건물지 2동, 우물 1기, 석렬 3기\n-고려 후기 건물 배치 양상 확인", period: "2021~2022", institution: "달빛역사연구원", source: "달빛역사연구원, 2024,\n『청운 별빛마을 건물지』", notes: "", pdf: "" },

        // ── 조선 (21) ─────────────────────────────────────────────
        Row { num: "60", era: "조선", era_span: 21, date: "전기", site: "청운 노을리 조선고분\n노을면 노을리 산98 일원", artifacts: "백자접시·종지·병·발, 청동숟가락 등", findings: "토광묘 4기", period: "2003~2004", institution: "청운문화재연구원", source: "청운문화재연구원, 2006,\n『청운 노을리 조선고분』", notes: "", pdf: "" },
        Row { num: "61", era: "조선", era_span: 0, date: "전기", site: "청운 수정동 조선유적\n수정동 136-1번지", artifacts: "수키와, 암키와, 도기 호, 청자 접시편, 분청사기 접시편, 백자 발, 철솥, 철도자, 철정 등", findings: "주거지 4기, 수혈 1기\n-평면형태 방형, 장방형\n-내부시설 구들, 부뚜막, 주혈 등", period: "2007", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2009,\n『청운 수정동 136-1번지 유적』", notes: "", pdf: "" },
        Row { num: "62", era: "조선", era_span: 0, date: "전기", site: "청운정\n솔밭면 솔밭리 산547", artifacts: "", findings: "조선시대 정자\n*향토유적 제8호\n-세종 때 처음 세워짐\n-한국전쟁 때 해체, 이후 복원", period: "1997", institution: "청운군지 편찬위원회", source: "청운군지편찬위원회, 1997,\n『청운군지』", notes: "", pdf: "" },
        Row { num: "63", era: "조선", era_span: 0, date: "전기", site: "청운 달빛리 분청사기 요지\n달빛면 산190 일원", artifacts: "분청사기 발, 접시, 종지, 잔, 병, 호, 합, 백자 등", findings: "가마 1기 확인\n*향토유적 제22호\n-반지하식 단실 등요\n-분청사기 초기단계 인화기법 주류\n-광주지역 가마와 유사", period: "2008", institution: "청운문화재연구원", source: "청운문화재연구원, 2010,\n『청운 달빛리 분청사기 요지 발굴조사보고서』", notes: "", pdf: "" },
        Row { num: "64", era: "조선", era_span: 0, date: "전기", site: "청운서원\n솔밭면 솔밭리 산210 일원", artifacts: "", findings: "조선 중기 인물을 기리는 서원\n-1652년 사우 창건\n-1714년 청운이라는 사액 받음\n-1871년 훼철, 1980년 복원", period: "2000~2001", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2002,\n『청운시 문화유적분포지도』", notes: "", pdf: "" },
        Row { num: "65", era: "조선", era_span: 0, date: "전기", site: "달빛서원\n달빛면 달빛리 산16-1 일원", artifacts: "", findings: "조선 후기 유학자를 기리는 서원\n-1638년 사우 창건\n-1868년 훼철, 1973년 복원시작\n-강당, 동재, 서재 등 복원", period: "2000~2001", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2002,\n『청운시 문화유적분포지도』", notes: "", pdf: "" },
        Row { num: "66", era: "조선", era_span: 0, date: "전기", site: "청운 노을리 봉수지\n노을면 노을리 봉화골 봉화뚝", artifacts: "병 구연부편, 도기편, 자기편 등", findings: "연조에 사용된 돌무지, 길이 18m 정도의 석축 추정건물지 확인\n해발 172m\n북쪽 솔밭봉수, 남쪽 달빛봉수와 응함", period: "1999", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2000,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "67", era: "조선", era_span: 0, date: "전기", site: "청운 바람리 봉수지\n바람골면 바람리 산86-1번지", artifacts: "회흑색 연질 토기 뚜껑, 연질토기편 등", findings: "평면 타원형 토축기단부의 봉수군 보호시설 조성, 4기의 석축연조 흔적\n-북쪽 노을봉수, 남쪽 별빛봉수 응함", period: "1999", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2000,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "68", era: "조선", era_span: 0, date: "전기", site: "청운 솔밭산 봉수지\n솔밭면 솔밭리 중군봉", artifacts: "기와편", findings: "산 정상의 평탄면 확인\n해발 230m\n북쪽 바람봉수-남쪽 노을봉수 연결", period: "1999", institution: "청운대학교 박물관", source: "청운대학교 박물관, 2000,\n『청운시 군사유적 지표조사 보고서』", notes: "지표조사", pdf: "" },
        Row { num: "69", era: "조선", era_span: 0, date: "전기", site: "청운 달빛리 흑유자 가마\n달빛면 달빛리 350-50번지 일원", artifacts: "흑유자, 백자, 초벌편, 요도구, 도기편 등 2,847점", findings: "자기가마 1기, 주거지 1기, 수혈유구 2기, 소성유구 4기, 구들 1기, 폐기장 2기, 석렬 1기\n-세장방형 가마\n-주변 유구는 가마 부속시설로 추정", period: "2004~2005", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2007,\n『청운 달빛리 흑유자 가마유적』", notes: "", pdf: "" },
        Row { num: "70", era: "조선", era_span: 0, date: "전기", site: "청운 별빛리 탄요 유적\n별내면 별빛리 산153번지 일원", artifacts: "도기편, 자기편 등 6점", findings: "탄요 2기\n-산지형 탄요, 원형의 반지하 등요", period: "2006", institution: "달빛역사연구원", source: "달빛역사연구원, 2008,\n『청운 별빛리 탄요 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "71", era: "조선", era_span: 0, date: "전기", site: "청운 운봉리 조선취락\n운봉면 운봉리 521-1번지 일원", artifacts: "백자, 도기장군, 흑갈유호, 청동숟가락, 청동합, 구슬, 철제가위 등", findings: "측구부탄요 1기, 회곽묘 4기, 토광묘 15기, 추정 석곽묘 1기\n-석곽묘는 조선시대 이전으로 불가 추정", period: "2009", institution: "청운문화재연구원", source: "청운문화재연구원, 2011,\n『청운 운봉리 조선취락유적』", notes: "", pdf: "" },
        Row { num: "72", era: "조선", era_span: 0, date: "중기", site: "청운 하늘재 건물지 유적\n하늘읍 하늘재리 산38번지 일원", artifacts: "청해파문·종선문 수키와, 청해파문 암키와, 백자 대접, 발, 접시, 철제도구 등", findings: "건물지 2동\n-1호 건물지 정면 2칸, 측면 1칸\n-2호 건물지 정면, 측면 2칸\n-민가 혹은 사찰 관련 가능성", period: "2012", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2014,\n『청운 하늘재 건물지 유적』", notes: "", pdf: "" },
        Row { num: "73", era: "조선", era_span: 0, date: "중기", site: "청운 구름동 조선유적(설운동 유물산포지)\n구름동 산34-8번지 일원", artifacts: "", findings: "조선시대 추정주거지 1기, 석축 1기, 시대미상 석곽고, 이장묘 1기\n-원형녹지보존지역으로 발굴유예", period: "2013", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2015,\n『청운 구름동 조선유적 시굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "74", era: "조선", era_span: 0, date: "중기", site: "청운 바람리 조선취락유적\n바람골면 바람리 389번지 일원", artifacts: "출토유물 없음", findings: "토광묘 1기\n-이단굴광 토광묘", period: "2015", institution: "달빛역사연구원", source: "달빛역사연구원, 2017,\n『청운 바람리 조선취락유적』", notes: "", pdf: "" },
        Row { num: "75", era: "조선", era_span: 0, date: "중기", site: "청운 노을리 회곽묘군\n노을면 노을리 초가팔리 389번지 일원", artifacts: "분청자접시편, 백자편 등", findings: "회곽묘 3기, 토광묘 7기, 수혈 2기, 구 1기", period: "2015~2016", institution: "청운문화재연구원", source: "청운문화재연구원, 2018,\n『청운 노을리 회곽묘군』", notes: "", pdf: "" },
        Row { num: "76", era: "조선", era_span: 0, date: "후기", site: "청운 달빛리 종가집터\n달빛면 달빛리 557번지 일원", artifacts: "명문 망새, 기하문 수막새, 파상문 수키와, 백자 제기접시·발, 청화백자 접시, 절구공이, 맷돌", findings: "건물지 4동(안채, 중문 및 광, 솟을대문 및 행랑마당, 사랑채), 담장, 구들\n-'ㄱ','ㅡ'자형 평면구조 건물\n-일반 종가에 비해 규모가 작음\n-조선 후기 건축양상", period: "2017", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2019,\n『청운 달빛리 종가집터 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "77", era: "조선", era_span: 0, date: "후기", site: "청운 독산봉수지\n하늘읍 하늘리 590번지 일원", artifacts: "수키와, 도기편, 분청사기편, 백자접시·발, 갈유자기편, 석환 등", findings: "봉수대(방호벽, 연대, 연조 3기, 고사, 망덕 등 확인)\n*향토유적\n-지표, 시굴조사만 진행\n-조선 전기부터 운영시작", period: "2018~2019", institution: "별빛문화유산연구원", source: "별빛문화유산연구원, 2021,\n『청운 독산봉수지』", notes: "", pdf: "" },
        Row { num: "78", era: "조선", era_span: 0, date: "후기", site: "청운 노을리 유적(1·2지점)\n노을면 노을리 291-2번지, 산23-1번지 일원", artifacts: "명문 암키와, 명문수키와편, 단경호, 청자완편, 백자 발, 엽전, 동경, 동전, 석제관옥 등", findings: "주거지 7기, 건물지 2기, 우물 1기, 수혈유구 35기, 토광묘 18기, 회곽묘 5기, 소성유구 3기", period: "2019", institution: "달빛역사연구원", source: "달빛역사연구원, 2021,\n『청운 노을리 유적 1·2지점』", notes: "", pdf: "" },
        Row { num: "79", era: "조선", era_span: 0, date: "후기", site: "청운 구름동 530-4번지 유적\n구름동 530-4번지 일원", artifacts: "암막새, 수키와, 암키와, 청자 접시, 백자 발·접시·종지·잔, 도기 호·소호, 상평통보, 철정, 편자 등", findings: "건물지 2기, 계단 1기, 축대 5기, 석열 2기\n-조선시대 청운현 관아터\n-계획적인 대지조성 확인\n-문지, 잡석지정, 초석, 온돌시설 확인\n-관아건물로 판단", period: "2020", institution: "청운문화재연구원", source: "청운문화재연구원, 2022,\n『청운 구름동 530-4번지 유적』", notes: "", pdf: "" },
        Row { num: "80", era: "조선", era_span: 0, date: "후기", site: "청운 은하리 479-2번지\n은하면 은하리 479-2번지 일원", artifacts: "출토유물 없음", findings: "탄요 1기, 수혈 5기", period: "2021~2022", institution: "하늘고고학연구소", source: "하늘고고학연구소, 2024,\n『청운 은하리 479-2번지 일원』", notes: "", pdf: "" },
    ]
}

fn build_store() -> HwpxStyleStore {
    let mut store = HwpxStyleStore::new();
    store.push_font(HwpxFont::new(0, "함초롬바탕", "HANGUL"));
    // CS0: normal 8pt
    let mut cs0 = HwpxCharShape::default();
    cs0.height = HwpUnit::from_pt(8.0).unwrap();
    store.push_char_shape(cs0);
    // CS1: header 8pt bold
    let mut cs1 = HwpxCharShape::default();
    cs1.height = HwpUnit::from_pt(8.0).unwrap();
    cs1.bold = true;
    store.push_char_shape(cs1);
    // CS2: small 7pt
    let mut cs2 = HwpxCharShape::default();
    cs2.height = HwpUnit::from_pt(7.0).unwrap();
    store.push_char_shape(cs2);
    // PS0: justify, 130% line spacing
    let mut ps0 = HwpxParaShape::default();
    ps0.alignment = Alignment::Justify;
    ps0.line_spacing_type = LineSpacingType::Percentage;
    ps0.line_spacing = 130;
    store.push_para_shape(ps0);
    // PS1: center, 130% line spacing
    let mut ps1 = HwpxParaShape::default();
    ps1.alignment = Alignment::Center;
    ps1.line_spacing_type = LineSpacingType::Percentage;
    ps1.line_spacing = 130;
    store.push_para_shape(ps1);
    store
}

/// A chunk of rows for one page, with recalculated era_span values.
struct Chunk {
    rows: Vec<Row>,
}

/// Splits data into page-sized chunks, respecting era boundaries when possible.
/// `max_rows` is computed dynamically by `estimate_max_rows()`.
fn chunk_data(data: &[Row], max_rows: usize) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current: Vec<Row> = Vec::new();
    let mut i: usize = 0;
    while i < data.len() {
        let era_start: usize = i;
        let mut era_end: usize = i + 1;
        while era_end < data.len() && data[era_end].era_span == 0 {
            era_end += 1;
        }
        let era_size: usize = era_end - era_start;

        if current.len() + era_size <= max_rows {
            for item in data.iter().take(era_end).skip(era_start) {
                current.push(item.clone());
            }
            i = era_end;
        } else if current.is_empty() {
            let take: usize = max_rows.min(era_size);
            for item in data.iter().skip(era_start).take(take) {
                current.push(item.clone());
            }
            current[0].era_span = take as u16;
            chunks.push(finalize_chunk(current));
            current = Vec::new();
            if era_size - take > 0 {
                i = era_start + take;
            } else {
                i = era_end;
            }
        } else {
            // Era doesn't fit whole — fill remaining space with partial era rows
            let remaining: usize = max_rows - current.len();
            if remaining > 0 {
                let take: usize = remaining.min(era_size);
                for item in data.iter().skip(era_start).take(take) {
                    current.push(item.clone());
                }
                i = era_start + take;
            }
            chunks.push(finalize_chunk(current));
            current = Vec::new();
        }
    }
    if !current.is_empty() {
        chunks.push(finalize_chunk(current));
    }
    chunks
}

/// Recalculate era_span values within a chunk.
fn finalize_chunk(mut rows: Vec<Row>) -> Chunk {
    // Rebuild era_span: walk through, find era groups within this chunk
    let mut i = 0;
    while i < rows.len() {
        let era = rows[i].era;
        let mut count = 1;
        while i + count < rows.len() && rows[i + count].era == era && rows[i + count].era_span == 0
        {
            count += 1;
        }
        rows[i].era_span = count as u16;
        // Clear era_span for continuation rows
        for j in 1..count {
            rows[i + j].era_span = 0;
        }
        i += count;
    }
    Chunk { rows }
}

// ── Automatic column splitting algorithm ─────────────────────────

/// Greedy left-to-right split: assigns columns to page 1 until the
/// accumulated min_width exceeds printable width, rest go to page 2.
fn split_columns(columns: &[ColumnDef], page_width_mm: f64) -> (Vec<usize>, Vec<usize>) {
    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();
    let mut accumulated: f64 = 0.0;
    let mut overflow: bool = false;

    for (i, col) in columns.iter().enumerate() {
        if !overflow && accumulated + col.min_width_mm <= page_width_mm {
            left.push(i);
            accumulated += col.min_width_mm;
        } else {
            overflow = true;
            right.push(i);
        }
    }
    (left, right)
}

/// Distributes remaining page width equally across all columns in the group.
fn distribute_widths(columns: &[ColumnDef], indices: &[usize], page_width_mm: f64) -> Vec<f64> {
    let total_min: f64 = indices.iter().map(|&i| columns[i].min_width_mm).sum();
    let bonus: f64 = if indices.is_empty() {
        0.0
    } else {
        (page_width_mm - total_min).max(0.0) / indices.len() as f64
    };
    indices.iter().map(|&i| columns[i].min_width_mm + bonus).collect()
}

/// Maps column index (0-10) to the corresponding Row field.
fn extract_field(row: &Row, col_index: usize) -> &str {
    match col_index {
        0 => row.num,
        1 => row.era,
        2 => row.date,
        3 => row.site,
        4 => row.artifacts,
        5 => row.findings,
        6 => row.period,
        7 => row.institution,
        8 => row.source,
        9 => row.notes,
        10 => row.pdf,
        _ => "",
    }
}

/// Estimates rendered line count for a cell, including word wrap.
/// Splits on `\n`, then for each line measures total visual width
/// accounting for full-width (Korean/CJK) and half-width (ASCII) characters.
fn estimate_cell_lines(text: &str, col_width_mm: f64) -> usize {
    let cell_margin_mm: f64 = 1.5; // left + right internal cell margins
    let usable_mm: f64 = (col_width_mm - cell_margin_mm).max(5.0);
    let full_width_mm: f64 = FONT_SIZE_PT * 0.3528; // Korean/CJK character width
    let half_width_mm: f64 = full_width_mm * 0.5; // ASCII/Latin character width

    let mut total_lines: usize = 0;
    for line in text.split('\n') {
        if line.is_empty() {
            total_lines += 1;
            continue;
        }
        // Measure total visual width of this line
        let mut line_width_mm: f64 = 0.0;
        let mut lines_for_segment: usize = 1;
        for ch in line.chars() {
            let w: f64 = if ch > '\u{FF}' { full_width_mm } else { half_width_mm };
            if line_width_mm + w > usable_mm {
                lines_for_segment += 1;
                line_width_mm = w;
            } else {
                line_width_mm += w;
            }
        }
        total_lines += lines_for_segment;
    }
    total_lines.max(1)
}

/// Computes the synchronized row height (in mm) for each data row,
/// considering ALL 11 columns across both facing pages.
/// Both left and right tables will use the same heights so rows stay aligned.
fn compute_row_heights(
    data: &[Row],
    left_indices: &[usize],
    right_indices: &[usize],
    left_widths: &[f64],
    right_widths: &[f64],
) -> Vec<f64> {
    let line_height_mm: f64 = FONT_SIZE_PT * 0.3528 * (LINE_SPACING_PCT as f64 / 100.0);
    let cell_padding_mm: f64 = 1.5;

    let col_widths: Vec<(usize, f64)> = left_indices
        .iter()
        .zip(left_widths.iter())
        .chain(right_indices.iter().zip(right_widths.iter()))
        .map(|(&idx, &w)| (idx, w))
        .collect();

    data.iter()
        .map(|row| {
            let max_lines: usize = col_widths
                .iter()
                .map(|&(idx, w)| estimate_cell_lines(extract_field(row, idx), w))
                .max()
                .unwrap_or(1);
            max_lines as f64 * line_height_mm + cell_padding_mm
        })
        .collect()
}

/// Computes how many rows fit on one page using pre-computed row heights.
fn rows_that_fit(row_heights: &[f64], page_height_mm: f64) -> usize {
    let line_height_mm: f64 = FONT_SIZE_PT * 0.3528 * (LINE_SPACING_PCT as f64 / 100.0);
    let cell_padding_mm: f64 = 1.5;
    let title_height_mm: f64 = 10.0 * 0.3528 * 1.3;
    let overhead_mm: f64 = title_height_mm + line_height_mm + (line_height_mm + cell_padding_mm);
    let available_mm: f64 = (page_height_mm - overhead_mm) * 0.97;

    let mut accumulated_mm: f64 = 0.0;
    let mut count: usize = 0;
    for &h in row_heights {
        if accumulated_mm + h > available_mm {
            break;
        }
        accumulated_mm += h;
        count += 1;
    }
    count.max(1)
}

/// Builds a Table from a subset of columns with distributed widths and
/// explicit row heights (synchronized across facing pages).
fn build_table_for_page(
    data: &[Row],
    col_indices: &[usize],
    widths_mm: &[f64],
    columns: &[ColumnDef],
    table_width_mm: f64,
    row_heights_mm: &[f64],
) -> Table {
    let widths_hwp: Vec<HwpUnit> =
        widths_mm.iter().map(|&w| HwpUnit::from_mm(w).unwrap()).collect();

    // Header row
    let header_cells: Vec<TableCell> = col_indices
        .iter()
        .zip(widths_hwp.iter())
        .map(|(&idx, &w)| header_cell(columns[idx].name, w))
        .collect();
    let header: TableRow = TableRow { cells: header_cells, height: None };

    // Data rows with synchronized heights
    let mut rows: Vec<TableRow> = vec![header];
    for (i, entry) in data.iter().enumerate() {
        let mut cells: Vec<TableCell> = Vec::new();
        for (&idx, &w) in col_indices.iter().zip(widths_hwp.iter()) {
            let text: &str = extract_field(entry, idx);
            let cs: usize = columns[idx].char_shape_idx;
            let ps: usize = columns[idx].para_shape_idx;
            cells.push(cell_lines(text, w, cs, ps));
        }
        let h: Option<HwpUnit> = row_heights_mm.get(i).map(|&mm| HwpUnit::from_mm(mm).unwrap());
        rows.push(TableRow { cells, height: h });
    }

    Table { rows, width: Some(HwpUnit::from_mm(table_width_mm).unwrap()), caption: None }
}

fn main() {
    println!("=== 청운시 문화유산 발굴연표 ===\n");
    std::fs::create_dir_all("temp").unwrap();
    let store = build_store();
    let all_data = data();
    let page = PageSettings::a4();
    let printable_width_mm = page.printable_width().to_mm();
    let printable_height_mm = page.printable_height().to_mm();

    // Step 1: Split columns across two pages
    let (left_indices, right_indices) = split_columns(&ALL_COLUMNS, printable_width_mm);
    let left_names: Vec<&str> = left_indices.iter().map(|&i| ALL_COLUMNS[i].name).collect();
    let right_names: Vec<&str> = right_indices.iter().map(|&i| ALL_COLUMNS[i].name).collect();
    println!("  좌측 컬럼 ({}): {:?}", left_indices.len(), left_names);
    println!("  우측 컬럼 ({}): {:?}", right_indices.len(), right_names);

    // Step 2: Distribute widths equally within each group
    let left_widths = distribute_widths(&ALL_COLUMNS, &left_indices, printable_width_mm);
    let right_widths = distribute_widths(&ALL_COLUMNS, &right_indices, printable_width_mm);

    // Step 3: Compute synchronized row heights (same for both tables)
    let row_heights =
        compute_row_heights(&all_data, &left_indices, &right_indices, &left_widths, &right_widths);
    let max_rows = rows_that_fit(&row_heights, printable_height_mm);
    println!("  페이지당 최대 행 수: {}", max_rows);

    // Step 4: Chunk data respecting era boundaries
    let chunks = chunk_data(&all_data, max_rows);
    println!(
        "  총 {}건 → {}개 청크 → {}개 섹션 (페이지 페어)",
        all_data.len(),
        chunks.len(),
        chunks.len() * 2
    );

    let mut doc = Document::new();
    let mut row_offset: usize = 0;
    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_label = if i == 0 {
            "청운시 문화유산 발굴연표".to_string()
        } else {
            format!("청운시 문화유산 발굴연표 ({})", i + 1)
        };

        // Slice row heights for this chunk
        let chunk_heights = &row_heights[row_offset..row_offset + chunk.rows.len()];
        row_offset += chunk.rows.len();

        // Left section (odd page)
        let left_table = build_table_for_page(
            &chunk.rows,
            &left_indices,
            &left_widths,
            &ALL_COLUMNS,
            printable_width_mm,
            chunk_heights,
        );
        println!(
            "  청크 {}: 좌 {}행×{}열, 우 {}행×{}열",
            i + 1,
            left_table.row_count(),
            left_table.col_count(),
            chunk.rows.len() + 1,
            right_indices.len()
        );
        let left_para = Paragraph::with_runs(
            vec![Run::table(left_table, CharShapeIndex::new(CS_NORMAL))],
            ParaShapeIndex::new(PS_NORMAL),
        );
        let left_section = Section::with_paragraphs(
            vec![p(&chunk_label, CS_HEADER, PS_CENTER), p("", CS_NORMAL, PS_NORMAL), left_para],
            PageSettings::a4(),
        );
        doc.add_section(left_section);

        // Right section (even page) — same heights as left
        let right_table = build_table_for_page(
            &chunk.rows,
            &right_indices,
            &right_widths,
            &ALL_COLUMNS,
            printable_width_mm,
            chunk_heights,
        );
        let right_label = format!("{} (계속)", chunk_label);
        let right_para = Paragraph::with_runs(
            vec![Run::table(right_table, CharShapeIndex::new(CS_NORMAL))],
            ParaShapeIndex::new(PS_NORMAL),
        );
        let right_section = Section::with_paragraphs(
            vec![p(&right_label, CS_HEADER, PS_CENTER), p("", CS_NORMAL, PS_NORMAL), right_para],
            PageSettings::a4(),
        );
        doc.add_section(right_section);
    }

    let validated = doc.validate().expect("validation failed");
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store).expect("encode failed");
    std::fs::write("temp/large_table.hwpx", &bytes).unwrap();
    println!("  ✅ temp/large_table.hwpx 생성 완료 ({} bytes)", bytes.len());
}
