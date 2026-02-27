//! 포천시 발굴연표 — wide table split across facing pages.
//!
//! Data from: 포천시 발굴연표 260222 1차최종.xlsx (106 entries)
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
        Row { num: "1", era: "구석기", era_span: 13, date: "중기구석기", site: "포천 냉정리 구석기유적\n관인면 냉정리 2, 408번지 일원", artifacts: "찌르개, 격지, 깨진 자갈돌, 부스러기 등 28점", findings: "석기 지표수습, 유물출토 문화층 미확인\n-표본조사 중 확인조사", period: "2008", institution: "국방문화재연구원", source: "국방문화재연구원, 2010,\n『포천 냉정리 구석기유적』", notes: "고려~조선시대 유물 지표수습", pdf: "" },
        Row { num: "2", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 용정리 구석기유적\n군내면 용정리 산15-96번지 일원", artifacts: "흑요석, 석영제 긁개, 밀개, 홈날, 톱니날, 뚜르개, 찌르개, 가로날도끼, 주먹대패, 여러면석기, 자르개, 몸돌, 망치, 격지 등 3,416점", findings: "중기~후기구석기 문화층 5개소 확인", period: "2010~2011", institution: "국강고고학연구소", source: "국강고고학연구소, 2013,\n『포천 용정리 구석기유적』", notes: "기존정리자료 누락자료", pdf: "" },
        Row { num: "3", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 용정리 유적(A구역)\n군내면 용정리·유교리 일원", artifacts: "몸돌, 타원형 주먹도끼, 주먹찌르개, 주먹대패, 망치, 모루, 밀개, 찌르개, 톱니날, 부리날, 뚜루개, 긁개, 홈날, 여러면석기, 격지 등 5,863점", findings: "중기~후기구석기 문화층 4개소 확인", period: "2012~2014", institution: "한백문화재연구원", source: "한백문화재연구원, 2016,\n『포천 용정리 유적-포천 용정일반산업단지 조성사업A구역 내 유적 시굴 및 A-1지점 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "4", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 용정리 유적(A구역 3지점)\n군내면 용정리 산16-22임 일원", artifacts: "몸돌, 격지, 주먹찌르개, 찍개, 여러면석기, 긁개, 홈날, 복합석기 등 354점", findings: "중기~후기구석기 문화층 4개소 확인", period: "2012~2014", institution: "고려문화재연구원", source: "高麗文化財硏究院, 2016,\n『抱川 龍井里遺蹟-포천 용정일반산업단지 조성사업 A구역 3지점 내 유적 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "5", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 용정리 유적(A구역 2지점)\n군내면 용정리 343-5번지 일원", artifacts: "찍개, 주먹도끼, 주먹찌르개, 여러면석기, 긁개, 밀개, 홈날, 부리날 등 2,638점", findings: "중기구석기시대 문화층 4개소, 후기구석기시대 문화층 1개소 확인", period: "2013~2014", institution: "예맥문화재연구원", source: "예맥문화재연구원, 2016,\n『抱川 龍井里遺蹟-포천 용정일반산업단지 조성사업 A구역 2지점 내 유적 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "6", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 용정리 유적(B구역)\n군내면 용정리 산15-31번지 일원", artifacts: "주먹도끼, 주먹찌르깨, 찍개, 뚜르개, 톱니날, 부리날, 홈날, 긁개, 밀개, 여러면석기, 복합석기 등", findings: "중기구석기 문화층 2개소 확인", period: "2013~2014", institution: "한강문화재연구원", source: "한강문화재연구원, 2016, 『포천 용정리 유적-포천 용정일반산업단지 조성사업 B구역 내 유적 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "7", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 화대리 쉼터 구석기유적\n일동면 화대리 386-3번지 일원", artifacts: "흑요석제 화살촉,슴베찌르개 등 119점", findings: "후기구석기 초기~중기 3개 문화층 확인", period: "2001~2002", institution: "강원대학교 유적조사단", source: "강원대핚교 유적조사단, 2005, 『抱川 禾垈里 쉼터 舊石器遺蹟』", notes: "", pdf: "" },
        Row { num: "8", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 동교동 구석기유적\n포천읍 동교동 56전 일원", artifacts: "안팎날찌개, 뚜르개, 몸돌, 부스러기, 깨진자갈돌 등 5점", findings: "시굴조사\n유물출토 문화층 미확인", period: "2009", institution: "국방문화재연구원", source: "국방문화재연구원, 2011,\n『포천 동교동 구석기유적』", notes: "", pdf: "" },
        Row { num: "9", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 대회산리 구석기유적\n영북면 대회산리 일원", artifacts: "몸돌, 격지, 조각, 자갈돌, 깨진자갈돌,\n돌날몸돌, 돌날, 찍개, 여러면석기, 주먹대패 등", findings: "문화층 3개소 확인", period: "2010~2013", institution: "국방문화재연구원", source: "국방문화재연구원,2015,\n『포천 대회산리 구석기유적』", notes: "", pdf: "" },
        Row { num: "10", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 중리 용수재울 유적\n관인면 중리 552번지 일원", artifacts: "몸돌, 돌날, 밀개, 긁개, 쐐기, 슴베찌르개, 갈돌, 갈판, 흑요석제 새기개, 쐐기 등 3,140점", findings: "문화층 2개소 확인", period: "2010~2013", institution: "겨레문화유산연구원", source: "겨레문화유산연구원, 2016,\n『포천 중리 용수재울 유적』", notes: "", pdf: "" },
        Row { num: "11", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 중리 늘거리 유적\n관인면 중2리 일원", artifacts: "주먹도끼, 흑요석, 밀개, 긁개, 찌르개, 좀돌날몸돌, 좀돌날, 응회암 밀개, 긁개, 돌날, 좀돌날, 좀돌날 몸돌 등 15,992점\n(문화층 출토 11,068점)", findings: "후기구석기시대 문화층 3개소 확인", period: "2010~2013", institution: "기호문화재연구원", source: "기호문화재연구원, 2016,\n『포천 중리 늘거리 유적』", notes: "", pdf: "" },
        Row { num: "12", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 운산리 뒷골 구석기유적\n창수면 운산리 133-1번지 일원", artifacts: "흑요석, 몸돌, 돌날몸돌, 세형돌날몸돌, 긁개, 끝날긁개, 새기개 등 4,232점", findings: "후기구석기 문화층 3개소 확인", period: "2011~2013", institution: "한국문화유산연구원", source: "韓國文化遺産硏究院, 2016,\n『抱川 雲山里 뒷골 舊石器遺蹟』", notes: "", pdf: "" },
        Row { num: "13", era: "구석기", era_span: 0, date: "중기구석기", site: "포천 신평리 유적\n신북면 신평리 214-1번지 일원", artifacts: "몸돌, 격지, 조각, 찍개, 여러면석기, 긁개, 홈날 등 18점", findings: "문화층 2개소 확인", period: "2012~2013", institution: "국강고고학연구소", source: "국강고고학연구소, 2015,\n『포천 신평리유적』", notes: "", pdf: "" },
        Row { num: "14", era: "신석기시대", era_span: 3, date: "", site: "포천 자작리유적Ⅱ\n자작동 250-3번지 일원", artifacts: "즐문토기편", findings: "시굴조사N2W1 트렌치 탐색갱 내 원형의 유구 윤곽선 확인\n-선사시대의 유구 존재가능성 확인,\n오랫동안 취락이 연속적으로 유지추정", period: "2003", institution: "경기도박물관", source: "경기도박물관, 2004,\n『抱川 自作里遺蹟Ⅱ』", notes: "n2e2", pdf: "" },
        Row { num: "15", era: "신석기시대", era_span: 0, date: "", site: "포천 중리 마산유적\n관인면 중리 303번지 일원", artifacts: "빗살무늬토기 구연부편", findings: "수혈유구 1기\n내부에 무시설식노지 1기, 중앙에 소형의 유구가 확인", period: "2011~2013", institution: "한국문화유산연구원", source: "한국문화유산연구원, 2015,\n『抱川 中里 馬山遺蹟』", notes: "", pdf: "" },
        Row { num: "16", era: "신석기시대", era_span: 0, date: "", site: "포천 거사리 신석기·원삼국시대 주거지\n영중면 거사리 370-10번지 일원", artifacts: "즐문토기 발, 구연부편, 저부편, 고석, 긁개, 갈돌, 몸돌 등 48점", findings: "주거지 1기\n-평면 방형, 내부시설 위석식 화덕시설, 바닥다짐 미확인\n-규모로 봤을 때 대형에 속함\n-다량의 탄화흔으로 볼 때 화재로 페기추정", period: "2015~2016", institution: "동국문화재연구원", source: "동국문화재연구원, 2018,\n『포천 거사리 신석기·원삼국시대 주거지』", notes: "", pdf: "" },
        Row { num: "17", era: "청동기", era_span: 13, date: "전기", site: "포천 선단동유적\n선단동, 가산면 마산리 일원", artifacts: "빗살무늬토기, 공렬토기, 무문토기,\n반달돌칼, 화살촉, 숫돌 등", findings: "주거지 4동", period: "2011~2013", institution: "국방문화재연구원", source: "국방문화재연구원, 2015,\n『포천 선단동 유적』", notes: "", pdf: "" },
        Row { num: "18", era: "청동기", era_span: 0, date: "전기", site: "포천 중리 마산유적Ⅱ\n관인면 중리 303번지 일원", artifacts: "무문토기 호, 석촉", findings: "주거지 3기\n-역삼동형 주거지 확인", period: "2014", institution: "서경문화재연구원", source: "서경문화재연구원, 2016,\n『포천 중리 마산유적Ⅱ』", notes: "", pdf: "" },
        Row { num: "19", era: "청동기", era_span: 0, date: "전기", site: "포천 만세교리 유적\n신북면 만세교리 94번지 일원", artifacts: "무문토기편, 갈돌, 대석 등 14점", findings: "주거지 1기\n-세장방형 주거지\n-생활공간보다는 다른 목적으로 이용되었을 가능성 제시", period: "2019", institution: "국강고고학연구소", source: "국강고고학연구소, 2021,\n『포천 만세교리유적』", notes: "", pdf: "" },
        Row { num: "20", era: "청동기", era_span: 0, date: "전기", site: "포천 중리 마산유적\n관인면 중리 303번지 일원", artifacts: "구순각목, 이중구연단사선문, 무문토기, 주상편인석부, 반월형석도", findings: "주거지 2기, 수혈유구 3기\n역삼동식 주거지 확인", period: "2011~2013", institution: "한국문화유산연구원", source: "한국문화유산연구원, 2015,\n『抱川 中里 馬山遺蹟』", notes: "", pdf: "" },
        Row { num: "21", era: "청동기", era_span: 0, date: "전기", site: "포천 오가리·주원리 유적\n창수면 오가리, 주원리 일원", artifacts: "신석기시대 토기편, 공열문토기, 무문토기, 반월형석도, 석촉, 석착, 지석 등", findings: "주거지 3기, 수혈 3기\n-북한강유형 혹은 송국리유형과 유사\n-내부시설이 없이 채집, 경작을 위한 임시거처로 추정", period: "2015~2016", institution: "고려문화재연구원", source: "高麗文化財硏究院, 2018,\n『抱川 伍佳里·注院里 遺蹟』", notes: "", pdf: "" },
        Row { num: "22", era: "청동기", era_span: 0, date: "전기", site: "포천 금현리 고인돌\n가산면 금현5리 304-10번지 일원", artifacts: "", findings: "덮개돌, 받침돌 3기\n*문화유산자료 제47호\n-탁자식\n-재질 화강암\n-남북방향", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "23", era: "청동기", era_span: 0, date: "전기", site: "포천 기산리 고인돌\n일동면 기산리 일원", artifacts: "", findings: "탁자식 지석묘 1기", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "24", era: "청동기", era_span: 0, date: "전기", site: "포천 만세교리 고인돌\n신북면 만세교리 일원", artifacts: "", findings: "지석묘 2기", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "25", era: "청동기", era_span: 0, date: "전기", site: "포천 선단동 고인돌\n선단동 일원", artifacts: "", findings: "지석묘 1기 외 석재노출", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "26", era: "청동기", era_span: 0, date: "전기", site: "포천 수입리 고인돌\n일동면 수입2리 8번지 일원", artifacts: "", findings: "탁자식 지석묘 2기\n*향토유적 제33호", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "27", era: "청동기", era_span: 0, date: "전기", site: "포천 자작동 고인돌\n자작동 251-2번지 일원", artifacts: "", findings: "탁자식 지석묘 1기\n*향토유적 제2호", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "28", era: "청동기", era_span: 0, date: "전기", site: "포천 추동리 고인돌\n창수면 추동2리 434-4번지 일원", artifacts: "", findings: "개석식 지석묘 3기", period: "2000~2006", institution: "경기도박물관", source: "경기도박물관, 2007,\n『경기도고인돌』", notes: "지표조사", pdf: "" },
        Row { num: "29", era: "청동기", era_span: 0, date: "전기", site: "포천 중리 늘거리 유적\n경기도 포천시 관인면 중2리 일원", artifacts: "무문토기 4점", findings: "고인돌 4기 확인\n탁자식, 개석기, 기반식 확인\n유아묘 존재 가능성 추정", period: "2010~2013", institution: "기호문화재연구원", source: "기호문화재연구원, 2016,\n『포천 중리 늘거리 유적』", notes: "", pdf: "" },
        Row { num: "30", era: "원삼국", era_span: 5, date: "1~2세기", site: "포천 길명리 유적\n일동면 길명리 374-3·4번지 일원", artifacts: "민무늬토기, 타날문토기편, 돌끌, 갈판, 미완성석기, 토우, 대롱옥 등", findings: "주거지 1기\n-평면형태 장방형, 내부시설 노비, 주혈", period: "2002", institution: "세종대학교 박물관", source: "세종대학교박물관, 2003,\n『포천 길명리』", notes: "", pdf: "" },
        Row { num: "31", era: "원삼국", era_span: 0, date: "1~2세기", site: "포천 금주리 유적\n영중면 금주리 1014-5번지 일원", artifacts: "무문토기편, 경질무문토기편, 타날문토기편, 화분형토기편, 토제방추차, 돌도끼, 석제그물추, 석제화살촉, 제작중석기, 숫돌, 철경동촉 등 155점", findings: "주거지 6기, 불탄자리 3기, 구상유구 6기\n-철자형, 장방형 주거지 확인\n-돌 위에 기둥을 세운 양상 확인", period: "2002~2003", institution: "세종대학교 박물관", source: "세종대학교박물관, 2005,\n『포천 금주리 유적』", notes: "", pdf: "" },
        Row { num: "32", era: "원삼국", era_span: 0, date: "1~2세기", site: "포천 구읍리 421번지 유적\n군내면 구읍리 421번지 일원", artifacts: "경질무문토기 구연부편, 기저부편", findings: "주거지 1기, 지상식건물지 1기\n-평면형태 장방형\n-내부시설 노지, 주혈", period: "2022", institution: "수도문물연구원", source: "수도문물연구원, 2024,\n『포천 구읍리 421번지 유적』", notes: "", pdf: "" },
        Row { num: "33", era: "원삼국", era_span: 0, date: "1~2세기", site: "포천 사정리 모래내유적\n관인면 사정리 35번지 일원", artifacts: "중도식무문토기, 타날문토기, 시루, 연질토기, 등잔형 토기, 토제방추차, 원형토제품, 지석, 구슬, 청동방울 등 148점", findings: "주거지 29기, 수혈유구 14기\n-구릉 정상부에 밀집분포 경향\n-凸자형 주거지 주류, 평면형태 육각형·오각형\n-내부시설 외줄구들, 부뚜막, 노지, 주공 등 확인\n-Ⅰ단계 마을형성, Ⅱ단계 가장 팽창됨, Ⅲ단계 취락 급속도 축소\n-백제의 팽창과도 관련 추정", period: "2014", institution: "중앙문화재연구원", source: "중앙문화재연구원, 2014,\n『抱川 射亭里 모래내遺蹟』", notes: "", pdf: "" },
        Row { num: "34", era: "원삼국", era_span: 0, date: "1~2세기", site: "포천 구읍리 유적(1지점)\n군내면 구읍리 291-2번지", artifacts: "경질무문토기 외반구연 옹, 호 등", findings: "주거지 3기, 수혈유구 2기, 주혈군 2기\n-구읍리 1지점에서 확인\n-주거지 평면형태 말각방형, 내부시설 'ㅡ','ㄱ'자형 부뚜막, 주혈 확인\n-주혈은 수혈유구 주변에서 확인되어\n울책 가능성", period: "2021", institution: "한국고고인류연구소", source: "한국고고인류연구소, 2021,\n『포천 구읍리 유적 1·2지점』", notes: "", pdf: "" },
        Row { num: "35", era: "원삼국~삼국", era_span: 11, date: "", site: "영송리 선사유적\n경기도 포천시 영중면 영송리 269 외", artifacts: "즐문토기편, 중도식외반구연호, 심발형토기, 승석문단경호, 호형토기, 무문토기시루편, 대옹, 대형토기옹, 토제방추차, 지석, 철모, 철도자, 철정 등", findings: "원삼국시대 주거지 5기, 미상수혈유구 1기\n-시도기념물 제 140호\n-포천지역 처음 이루어진 발굴조사\n-중소형의 철자형주거지 확인\n-외줄구들의 부뚜막 확인\n-부뚜막에 토기가 걸\n-유적 주변으로 구석기, 신석기시대 토기가 수습되어 원삼국시대 이전부터 마을의 존재가 확임", period: "1994", institution: "한양대학교 박물관", source: "한양대학교 박물관, 1995,\n『영송리 선사유적』", notes: "", pdf: "" },
        Row { num: "36", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 성동리 마을유적\n영중면 성동5리 일원", artifacts: "즐문토기편, 단경호, 타날문토기편, 심발형토기편, 회청색경질토기편, 대부완, 동이, 고배, 외반구연옹, 기대, 주조철부편, 철편, 철촉, 철겸, 소찰 등 808점", findings: "원삼국시대 주거지 1기, 한성백제 주거지 4기, 소형유구 12기, 구상유구 1기, 신라 주거지 2기, 소혈유구 21기, 조선시대 소혈유구 2기(민묘), 시대미상 소형유구 8기\n-성동리산성과 연관된 유적\n-한성백제, 신라시대 유구의 분포양상 구분\n-신라의 사민정책이 반영\n-규모가 작고, 토기가 방치된 것으로 모아 단기적 목적의 마을로 추정\n-출토유물의 양상으로 볼 때, 신석기, 원삼국, 삼국시대까지 이어짐", period: "1998", institution: "경기도박물관", source: "京畿道博物館, 1999,\n『抱川 城洞里 마을遺蹟』", notes: "", pdf: "" },
        Row { num: "37", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 자작리유적Ⅰ\n포천읍 자작리 251-2번지 일원", artifacts: "격자·사격자문·무문 암키와, 대옹, 대호, 직구호, 동이, 원저단경호, 장란형토기, 심발형토기, 원통형기대, 주구부토기, 원통형시루, 뚜껑, 타날문토기편, 경질무문토기편, 동진제중국청자편, 철제소찰, 철정, 꺽쇠, 철도자, 철촉편, 지석, 토제어망추, 토제품 등", findings: "주거지 2기, 소형유구 6기, 구상유구 4기, 굴립주건물지 1기\n-주거지 평면형태 여철자형 주거지\n-대형 주거지\n-통형기대, 동진대 중국청자, 다량의 기와 등으로 볼 때 위계가 높은 것으로 추정\n-슬레그, 장고형이기재 등 자체적인 철기 및 토기 생산 가능성 추정", period: "2000~2001", institution: "경기도박물관", source: "경기도박물관, 2004,\n『抱川 自作里遺蹟Ⅰ』", notes: "", pdf: "" },
        Row { num: "38", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 자작리유적Ⅱ\n자작동 250-3번지 일원", artifacts: "대옹, 원통형기대편, 소형기대편, 원저단경호, 타날문토기편, 경질무문토기편, 철촉편, 철도자편, 지석 등", findings: "주거지 40~43기 존재 추정, 소형유구 81~84기 내외, 구상유구 4기, 굴립주건물지 4기 등\n-시굴조사로 유적의 범위 확인\n-내부조사 미진행\n-선사시대~삼국시대까지의 연속적\n으로 사용된 취락으로 추정\n-백제시대 주거지의 비율이 높음", period: "2003", institution: "경기도박물관", source: "경기도박물관, 2004,\n『抱川 自作里遺蹟Ⅱ』", notes: "", pdf: "" },
        Row { num: "39", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 중리 용수재울 유적\n포천시 관인면 중리 552번지 일원", artifacts: "경질무문토기, 타날문토기, 이형토기, 회색무문양토기, 토제품, 석제품, 마노구슬 철촉, 환두소도 등", findings: "주거지 32기, 수혈유구 55기, 소성유구 6기, 고상가옥 7기, 목책 4기, 구상유구 1기\n- 포천 동북부 취락유적 중 최대 규모\n- 凸자형 주거지 주류\n- 낙랑계 기술영향이 확인되고, 백제계 토기 영향이 적음", period: "2010~2013", institution: "겨레문화유산연구원", source: "겨레문화유산연구원, 2016,\n『포천 중리 용수재울 유적』", notes: "", pdf: "" },
        Row { num: "40", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 중리 마산유적\n관인면 중리 303번지 일원", artifacts: "경질무문토기, 대옹, 타날문토기, 장란형토기, 원저호, 철도자, 철경동촉 등", findings: "주거지 10기, 수혈유구 3기\n-주거지의 평면형태(오각형→육각형)와 쪽구들(ㄱ자형→ㅡ자형) 등 변화양상이 확인되나 여러가지 유형이 취사선택되어 공존되는 것으로 추정\n-경질무문토기가 주를 이루고, 타날문토기는 한정적", period: "2011~2013", institution: "한국문화유산연구원", source: "한국문화유산연구원, 2015,\n『抱川 中里 馬山遺蹟』", notes: "", pdf: "" },
        Row { num: "41", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 자작리유적Ⅲ\n자작동 250-3번지 일원", artifacts: "수키와편, 암키와편, 경질무문토기 옹·완·뚜껑·발·소호·분형토기, 장란형토기, 심발형토기, 단경호, 회색무문양토기 호, 직구단경호, 단경호, 직구호, 광구단경호, 대옹, 마연토기편, 시루, 이중구연호, 기대편, 원저단경호, 원저호,동이편, 도자편, 주조괭이, 철정, 쇠스랑, 숫돌, 갈돌, 원형토제품, 방추차 등", findings: "주거지 25기, 지상식건물지 1기, 수혈83기, 구상유구 7기\n-주거지 평면형태 육각형, 오각형, 방형·원형 확인, 凸, 呂자형 돌출된 출입구의 평면 육각형 주거지가 주\n-내부시설 평면형태 'ㄱ','ㅡ'자형 부뚜막, 주혈\n-벽체 기둥을 세운 후 점토를 덧대어 벽체를 세운 후, 보강 위해 판재를 덧댐, 바닥 점토다짐 혹은 불처리\n-출입구 시설 확인된 것은 18기로 남쪽 혹은 남동쪽으로 배치\n-수혈 수 기가 소군집을 이루며 분포, 규모가 다양하게 확인. 대옹, 주조철부, 벼이삭, 벽면보강흔 등으로 볼 때 저장용 수혈로 추정\n-출토유물, 주변 반월산성, 고모리산성 등으로 볼 때 거주집단의 위계와 역할이 중요했던 것으로 분석", period: "2013", institution: "기호문화재연구원", source: "기호문화재연구원, 2015,\n『포천 자작리 유적Ⅲ』", notes: "", pdf: "" },
        Row { num: "42", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 중리 마산유적Ⅱ\n관인면 중리 303번지 일원", artifacts: "심발형토기, 평저호, 경질무문토기편\n원형석제품", findings: "주거지 1기, 수혈유구 1기, \n시대미상 수혈유구 1기 확인\n-오각형주거지, ㄱ자형 쪽구들", period: "2014", institution: "서경문화재연구원", source: "서경문화재연구원, 2016,\n『포천 중리 마산유적Ⅱ』", notes: "", pdf: "" },
        Row { num: "43", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 거사리 신석기·원삼국시대 주거지 유적\n영중면 거사리 370-10번지 일원", artifacts: "경질무문토기 호·소호·천발, 방추차, 타날문토기, 석촉편, 철도자, 능형철판 등 58점", findings: "원삼국시대 주거지 3동, 수혈 1기\n-오각형주거지+ㄱ자형 주거지, 육각형주거지 확인\n-백제 이전 중소규모 마을의 한 형태\n-생산 또는 작업공간 등 특수한 목적과 연관될 가능성 존재", period: "2015~2016", institution: "동국문화재연구원", source: "동국문화재연구원, 2018,\n『포천 거사리 신석기·원삼국시대 주거지』", notes: "", pdf: "" },
        Row { num: "44", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 오가리·주원리 유적\n창수면 오가리, 주원리 일원", artifacts: "경질무문토기, 타날문토기편", findings: "(주원리 1지점) 경작유구 3기\n-비교적 소규모 경작\n-동일한 층에서 경질무문토기 출토 수혈로 볼 때 원삼국시대로 추정", period: "2015~2016", institution: "고려문화재연구원", source: "高麗文化財硏究院, 2018,\n『抱川 伍佳里·注院里 遺蹟』", notes: "", pdf: "" },
        Row { num: "45", era: "원삼국~삼국", era_span: 0, date: "", site: "포천 자작리유적Ⅳ\n자작동 250-4번지 일원", artifacts: "타날문토기 대옹·대호·단경호·호·옹, 장란형토기, 심발형토기, 시루편, 동이편, 봉상형파수부, 경질무문토기 외반구연 옹, 철도자편, 철촉편, 동착, 토제방추차편, 반월형석도 등", findings: "주거지 6기, 수혈 34기, 굴립주건물지 1기\n-주거지 출입구 呂,凸자형, 생활공간 오각 또는 육각\n-내부시설 'ㅣ'자형 구들시설, 주혈, 생활면 다짐 확인\n-수혈유구 저장용으로 추정\n-굴립주건물지 전면 2칸·측칸 2칸, 북쪽으로 출입구 설치 추정, 주거지 출입구 남동-북서와 대조적", period: "2023", institution: "화서문화유산연구원", source: "화서문화유산연구원, 2025,\n『포천 자작리 유적Ⅳ』", notes: "", pdf: "" },
        Row { num: "46", era: "삼국", era_span: 5, date: "", site: "포천 고소산성\n창수면 고소성리 산2-1번지 일원", artifacts: "토기편 등", findings: "테뫼식 석축산성\n둘레 444m\n토기편으로 볼 때, 삼국시대로 추정", period: "1997", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사", pdf: "" },
        Row { num: "47", era: "삼국", era_span: 0, date: "", site: "포천 소고산성\n창수면 주원리 할미산", artifacts: "불명철기편, 토기편 등", findings: "테뫼식 석축산성\n둘레 87m\n막돌허튼층쌓기\n소규모 보루로 교통로 차단 목적 추정", period: "1997", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사", pdf: "" },
        Row { num: "48", era: "삼국", era_span: 0, date: "", site: "포천 할미산성\n관인면 냉정리 산 225-6 일원", artifacts: "고배편, 인화문토기편, 토기편, 기와편 등", findings: "원형의 석축산성\n-해발 200m\n-둘레 250m\n-서쪽성벽 일부 성벽 잔존 확인\n-성내평탄지 없어 시설물 없을것으로\n추정, 단기적방어를 위한 성 추정", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 鐵原郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사\n철원경계맞물림", pdf: "" },
        Row { num: "49", era: "삼국", era_span: 0, date: "", site: "포천 고모리산성\n소흘읍 고모리 산61-1번지 일원", artifacts: "개배, 고배, 완, 뚜껑, 합, 심발형토기, 자란형토기, 호·옹편, 원통형파수부, 삼족기 저부편, 타날문토기편, 중국제 시유도기편 등", findings: "동벽일대 석축성벽, 수혈유구\n-2단의 석축부, 기초부만 잔존\n-토축성벽을 석축성벽으로 개축\n-출토유물이 다수 수습되는 것으로\n볼 때, 성 내 정상부 중심에 생활시설\n잔존 가능성 높음", period: "2016~2017", institution: "한백문화재연구원", source: "한백문화재연구원, 2019,\n『포천 고모리산성Ⅰ』", notes: "", pdf: "" },
        Row { num: "50", era: "삼국", era_span: 0, date: "", site: "포천 구읍리 유적(1·2지점)\n군내면 구읍리 291-2번지, 산23-1번지 일원", artifacts: "단경호, 완, 호, 파수부호, 파수부완, 대부완, 옹, 봉상철기, 병, 고배, 부가구연대부장경호, 팔각병, 암키와편, 철제테두리 등", findings: "수혈주거지 6기, 수혈 8개, 주혈군 3개소, 석실묘 4기, 석곽묘 4기, 구상유구 2기\n-주거지 평면형태 방형, 장방형, 凸자형 등, 내부시설 노지, 저장수혈, 주혈 등\n-반월산성에 부속된 농경취락 가능성\n-석실묘, 석곽묘 평면형태 대부분 장방형을 이룸, 부분적으로 시상대 확인.\n-2지점에서만 분묘 확인", period: "2021", institution: "한국고고인류연구소", source: "한국고고인류연구소, 2021,\n『포천 구읍리 유적 1·2지점』", notes: "원삼국?", pdf: "" },
        Row { num: "51", era: "삼국~조선", era_span: 1, date: "", site: "포천 반월성[포천 구읍리 반월산성(1차)]\n경기도 포천군 군내면 구읍리 산 5-1번지 일원", artifacts: "마홀수해공구전 명문기와, 수막새, 귀면와, 무문·직선문·사선문·격자문·사격자문 기하문·화문·초화문·파도문·타원문·복합문·어골문평기와, 장동호, 호, 평저토기, 광구호, 단경호, 완, 대부발, 접시, 토기 뚜껑, 벼루편, 고배, 구절판편, 파수부편, 철제도끼, 철겸, 철제과대·요대, 철정, 방추차, 어망추, 시루편, 홈자귀, 숫돌, 사다리꼴 석재,  등", findings: "장대지, 건물지 3기, 서치성, 성벽 단면, 북문지 조사\n*사적\n-장대지, 건물지의 대지조성, 적심석, 기단석렬 등 확인\n-체성벽 수직에 가깝게 품자형 축조\n-체성 축조 후 치성 축조\n-북문지 평거식 확인, 강회사용 확인", period: "1995", institution: "단국대학교 문과대학 사학과", source: "단국대학교 문과대학 사학과,\n1996, 『포천 반월산성 1차 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "52", era: "통일신라", era_span: 1, date: "9세기", site: "포천 동교동 419-32번지 유적\n동교동 419-32번지 일원", artifacts: "적갈색 연질토기 완", findings: "수혈식석곽묘 1기\n-비교적 대형에 속하는 것\n-내부시설 시상", period: "2020", institution: "수도문물연구원", source: "수도문물연구원, 2022,\n『포천 동교동 419-32번지 유적』", notes: "", pdf: "" },
        Row { num: "53", era: "통일신라~고려", era_span: 1, date: "", site: "포천 용정리 유적(B구역)\n군내면 용정리 산15-31번지 일원", artifacts: "암키와편, 수키와편, 수막새편", findings: "기와가마 3기, 용도미상가마 1기", period: "2013~2014", institution: "한강문화재연구원", source: "한강문화재연구원, 2016, 『포천 용정리 유적-포천 용정일반산업단지 조성사업 B구역 내 유적 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "54", era: "통일신라~조선", era_span: 1, date: "", site: "포천 계류리 유적\n신북면 계류리 373-2전 일원", artifacts: "기와편, 토기 호, 대호, 도기뚜껑, 편병편, 완, 시루 바닥편, 호, 매병편, 평저소병, 대부병, 소호, 청자화형접시, 분청접시·대접, 백자발, 철솥 편, 철제괭이, 방형과판, 배목과 사슬 등", findings: "통일신라~고려 주거지, 삼가마, 구들,\n소성유구 2기, 석렬 2기, 통일신라~조선 65기 등\n-주거지 평면형태 장방형·방형, 내부시설 구들 'ㄱ''ㅡ'자형, 벽체시설, 주혈 \n-삼가마는 분리형 1기, 일체형 2기\n-수혈유구 소형의 원형이 대부분, 일부 2~4m 대형 수혈유구 확인", period: "2014", institution: "기호문화재연구원", source: "기호문화재연구원, 2016,\n『포천 계류리 유적』", notes: "", pdf: "" },
        Row { num: "55", era: "고려", era_span: 3, date: "", site: "포천 보가산성\n관인면 중리 산251-1번지 일원", artifacts: "기와편", findings: "포곡식 석축산성\n-전체 둘레 4.2㎞\n-성문지, 수구부, 추정건물지 등 확인\n-궁예와 관련 문헌기록 존재\n-쐐기돌을 많이 사용하고, 석재의 크기가 일정하지 않아 전반적으로 치졸하여 고려시대에 축조 추정", period: "1995", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1995, 『京畿道 漣川郡 軍事遺蹟 地表調査 報告書』", notes: "연천에 걸쳐있음.", pdf: "" },
        Row { num: "56", era: "고려", era_span: 0, date: "", site: "포천 운악산성\n화현면 산202번지 일원", artifacts: "어골문, 격자문 기와편, 회청색·회갈색 경질통기", findings: "편축과 협축을 혼용한 석축산성\n-해발 935m\n-성벽은 험준한 지형을 활용하여 축조, 북쪽과 남쪽 일부구간만 축조\n-성돌은 화강암계통을 성돌을 이용하여 내탁, 혹은 협축으로 축조\n-내부에서 추정문지, 탄요의 흔적 등이 확인\n-입보형 성곽 추정", period: "1997", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "가평에 걸쳐있고, 포천에는 등록되어 있지 않음.\n일대에 대한 지표조사 보고서가 언급되나 실제자료 확인되지 않음.", pdf: "" },
        Row { num: "57", era: "고려", era_span: 0, date: "", site: "송우리 태봉\n소흘읍 송우리 산 28-2 일원", artifacts: "", findings: "태조 왕건 소생 정희왕녀 아기의 재를 묻은 곳\n-태항은 도굴, 석대와 개석만 잔존\n-개석은 최근에 만들어진 것", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "58", era: "고려~조선", era_span: 6, date: "1173년", site: "포천향교\n군내면 구읍리 176", artifacts: "", findings: "외삼문, 내삼문, 명륭당, 대성전 등\n-문화유산자료 제47호\n-1173년 창건, 1591년 소실, 1594년\n중건, 1962년 중수, 1984년 보수", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "지표조사", pdf: "" },
        Row { num: "59", era: "고려~조선", era_span: 0, date: "1173년", site: "포천 추동리유적\n창수면 추동리 산 70-11 일원", artifacts: "토기편, 도기 호, 백자종지, 주조괭이, 물미, 쇠삽날, 철검, 철촉, 철정, 갈돌공이 등", findings: "수혈건물지 2기, 수혈유구 1기, 바위 등\n-2열의 구들열, 아궁이, 배연시설 확인\n-수혈건물지 1호 내 철기유물이 다량 확인되어 조선시대 화전민 거주 추정\n-2지구 시굴조사에서 수혈유구 4기, 구상유구 3기 등 확인, 유물미확인", period: "2011", institution: "국방문화재연구원", source: "국방문화재연구원, 2013\n『포천 추동리유적』", notes: "", pdf: "" },
        Row { num: "60", era: "고려~조선", era_span: 0, date: "1173년", site: "포천 중리 마산유적\n관인면 중리 303번지 일원", artifacts: "분청자, 백자, 동이, 도기편, 기와편 등", findings: "고려~조선시대 건물지 1기,\n조선시대 주거지 2기, 수혈유구 15기,\n유물지 1기", period: "2011~2013", institution: "한국문화유산연구원", source: "한국문화유산연구원, 2015,\n『抱川 中里 馬山遺蹟』", notes: "", pdf: "" },
        Row { num: "61", era: "고려~조선", era_span: 0, date: "1173년", site: "포천 선단동유적\n선단동, 가산면 마산리 일원", artifacts: "청자접시 및 완, 백자접시 및 병,\n도기 완, 시루, 평기와, 철부, 철정, 청동숟가락 등", findings: "조선시대 건물지 3동, 흑탄요 1기, 소성유구 1기, 집석유구 2기, 수혈유구 4기, 토광묘 4기, 회곽묘 1기, 시대미상 수혈유구 3기, 토광묘 4기\n-출토유물로 볼때 고려 말~조선 후기\n-3지점 2호 토광묘 내 숯은 회를 대체했던 것으로 추정되는 조선 후기 모제양식의 변화", period: "2011~2013", institution: "국방문화재연구원", source: "국방문화재연구원, 2015,\n『포천 선단동 유적』", notes: "", pdf: "" },
        Row { num: "62", era: "고려~조선", era_span: 0, date: "1173년", site: "포천 용정리 유적(B구역)\n군내면 용정리 산15-31번지 일원", artifacts: "암키와편, 수키와편", findings: "건물지 2기, 석렬 2기, 집석 1기, 토광묘 6기, 회곽묘 8기, 구상유구 2기, 수혈 7기", period: "2013~2014", institution: "한강문화재연구원", source: "한강문화재연구원, 2016, 『포천 용정리 유적-포천 용정일반산업단지 조성사업 B구역 내 유적 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "63", era: "고려~조선", era_span: 0, date: "1173년", site: "포천 거사리 유적\n영중면 거사리 산 82임 일원", artifacts: "분청자기 호편, 청자발·접시·호·병·잔·매병·마상, 초벌 저부편, 흑유주자, 백자명기, 갓모, 도침, 시침, 지석 동전, 봇극, 관정, 고리, 수저, 가위 등", findings: "고려시대 청자가마 1기, 폐기장 2기, 석곽묘 1기, 수혈유구 3기, 조선시대 숯가마 4기, 토광묘 8기, 회곽묘 3기, 수혈유구 1기", period: "2019~2020", institution: "국방문화재연구원", source: "국방문화재연구원, 2022,\n『포천 거사리 유적』", notes: "", pdf: "" },
        Row { num: "64", era: "조선", era_span: 43, date: "전기", site: "포천 고모리 유적\n고모리 산98 일원", artifacts: "백자접시·종자·병·발, 청동숟가락 등", findings: "토광묘 5기", period: "2020~2023", institution: "국방문화재연구원", source: "국방문화재연구원, 2025,\n『포천 고모리 유적』", notes: "", pdf: "" },
        Row { num: "65", era: "조선", era_span: 0, date: "전기", site: "포천 무봉리 유적\n소흘읍 무봉리 136-1번지", artifacts: "수키와, 암키와, 도기 호, 청자 접시편, 청자상감국화문 저부편, 분청사기 접시편, 백자 발, 철솥, 철도자, 철정, 문고리 등", findings: "주거지 5기, 수혈 1기\n-평면형태 방형, 장방형\n-내부시설 구들, 부뚜막, 주혈 등", period: "2021", institution: "국토문화재연구원", source: "국토문화재연구원, 2021,\n『포천 무봉리 136-1번지 유적』", notes: "", pdf: "" },
        Row { num: "66", era: "조선", era_span: 0, date: "전기", site: "금수정\n창수면 오가리 산547", artifacts: "", findings: "조선시대 정자\n-향토유적 제17호\n-한국전쟁 때 해체, 이후 장초석만 남아있는 결 현재의 모습으로 복원\n-세종떄 처음 세워짐, 우두정.", period: "1997", institution: "포천군지 편찬위원회", source: "포천군지편찬위원회, 1997,\n『포천군지』", notes: "", pdf: "" },
        Row { num: "67", era: "조선", era_span: 0, date: "전기", site: "포천 화현리 분청사기 요지\n화현면 산 190 일원", artifacts: "분청사기 발, 접시, 종지, 잔, 병, 호, 마상배, 장군, 대발, 삼족기, 합, 백자 등", findings: "가마 1기 확인\n-향토유적 제52호\n-반지하식 단실 등요,\n-요전부, 연소실, 번조실, 배연부 등\n-분청사기 초기단계의 국화문, 육각문, 삼원문 등 단독 인화기법이 주류\n-광주지역 분청사기 가마와 유사", period: "2005", institution: "육군사관학교\n화랑대연구소", source: "육군사관학교 화랑대연구소,\n2006, 『포천 화현리 분청사기 요지 발굴조사보고서』", notes: "", pdf: "" },
        Row { num: "68", era: "조선", era_span: 0, date: "전기", site: "옥병서원\n창수면 주원리 산210 일원", artifacts: "", findings: "박순(1523~1589) 등을 기리는 서원\n-1649년 사우 창건, 1713년 옥병이라는\n사액 받음.\n-1871년 훼철\n-1978년 복원 시작하여 숭현각, 삼문, 담장, 창옥재, 송월당 및, 홍살문 등 복원", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "69", era: "조선", era_span: 0, date: "전기", site: "화산서원\n가산면 방축리 산16-1 일원", artifacts: "", findings: "이항복 등을 기리는 서원\n-1631년 사우 창건, 1635년 현재의 위치로 이전\n-1868년 훼철\n-1971년 복원시작하여 인덕전, 동강재, 필운재 등 복원", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "70", era: "조선", era_span: 0, date: "전기", site: "용연서원-동·서재\n신북면 신평2리 165번지 일원", artifacts: "연화문 수막새, 호상집선문·복합집선문 수키와·교차집선문 수키와, 무문암키와, 호, 동이, 백자 접시·종지, 중국청화백자, 철화백자 호편, 청화백자 발 등", findings: "건물지 2동 확인\n동·서재로 추정되는 건물지 위치확인", period: "2007", institution: "한백문화재연구원", source: "한백문화재연구원, 2009,\n『포천 용연서원-동·서재』", notes: "", pdf: "" },
        Row { num: "71", era: "조선", era_span: 0, date: "전기", site: "포천 길명리 유적\n일동면 길명리 374-3·4번지 일원", artifacts: "토기편, 백자접시편 등", findings: "불탄유구 3기, 돌유구\n-집터 혹은 온돌, 아궁이 관련 시설 추정", period: "2002", institution: "세종대학교 박물관", source: "세종대학교박물관, 2003,\n『포천 길명리』", notes: "", pdf: "" },
        Row { num: "72", era: "조선", era_span: 0, date: "전기", site: "할미산 봉수지\n관인면 냉정리 산 225-6", artifacts: "", findings: "돌이 많이 확인되는 남봉우리 일대로 추정\n-해발 200m\n-18세기 이후 축조\n-서북쪽 소이산봉수대, 동북쪽 상사봉\n봉수대, 남 적골산봉수대 대응\n-할미산성 내 정상부 위치", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 鐵原郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사\n철원지역으로 표시", pdf: "" },
        Row { num: "73", era: "조선", era_span: 0, date: "전기", site: "청성사\n신북면 가채리 산23-2", artifacts: "", findings: "최치원을 봉향하는 사당\n-시도유형 문화유산 제64호\n-1768년 최성묵 등의 발기로 건립\n-1935년 현재 위치로 이전\n-삼문, 사당 등의 건물로 구성", period: "", institution: "", source: "", notes: "발굴x", pdf: "" },
        Row { num: "74", era: "조선", era_span: 0, date: "전기", site: "포천 이동면 노곡리 탄요 유적\n이동면 노곡리 산153번지 일원", artifacts: "도기편, 자기편 등 4점", findings: "탄요 2기\n-산지형 탄요, 원형의 반지하 등요", period: "2004", institution: "명지대학교 박물관", source: "명지대학교박물관, 2006,\n『포천 이동면 노곡리 탄요 발굴조사 보고서』", notes: "", pdf: "인쇄물only" },
        Row { num: "75", era: "조선", era_span: 0, date: "전기", site: "포천 길명리 흑유자 가마\n-일동면 길명리 350-50번지 일원", artifacts: "흑유자, 백자, 초벌편, 요도구, 도기편 등 3348점", findings: "자기가마 1기, 주거지 1기, 수혈유구 3기, 소성유구 5기, 구들 1기, 적석유구 1기, 노지 1기, 폐기장 2기, 석렬 1기\n-세장방형 가마, 아궁이 일부 및 5개의 소성실, 연도부 잔존\n-주변 유구는 가마의 생산과 관련된 \n부속시설로 추정", period: "2002~2003", institution: "기전문화재연구원", source: "畿甸文化財硏究院, 2006,\n『抱川 吉明里 黑釉姿窯址』", notes: "", pdf: "" },
        Row { num: "76", era: "조선", era_span: 0, date: "전기", site: "성동리 태봉 석조물\n영중면 성동리 668-1도 일원", artifacts: "", findings: "추존왕 익존 태봉\n-성동리 주변 흩어진 석재를 수습하여 영평천가 도로변으로 이동\n-탑신석, 태실, 개석, 귀부, 하마비, 사다리꼴석재, 연자방아 석재 등", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "77", era: "조선", era_span: 0, date: "전기", site: "권종 충신문\n소흘읍 고모리 32번지", artifacts: "", findings: "맞배지붕의 건물\n-향토유적 제40호", period: "1998", institution: "단국대학교 사학과", source: "단국대학교 사학과, 1998,\n『포천군의 역사와 문화유적』", notes: "", pdf: "" },
        Row { num: "78", era: "조선", era_span: 0, date: "전기", site: "안동김씨 종가집터\n창수면 오가리 557번지 일원", artifacts: "명문 망새, 기하문 수막새, 파상문 수키와, \n무문·복합선문 암키와, 백자 제기접시·발,\n잔, 회청사기 굽편, 청화백자 접시, 절구공이,맷돌", findings: "건물지 4동(안채, 중문 및 광, 솟을대문 및 행랑마당, 사랑채), 담장, 구들등\n-'ㄱ', 'ㅡ'자형 평면구조 건물 확인\n-건물들이 일정한 규격에 의해 배치\n-공간들의 구분 확인\n-일반 종가에 비해 규모가 작음\n-조선 후기 건축양상이 나타나며,\n최소한 공간과 건물로 종가집을 구성", period: "2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2005, 『포천 안동김씨 종가집터 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "79", era: "조선", era_span: 0, date: "전기", site: "영송리 선사유적\n경기도 포천시 영중면 영송리 269 외", artifacts: "녹청자편, 백자대접·사발·접시·연수기·종지·완·잔, 암키와편, 철제월, 철제낫, 철제호미 등", findings: "건물지유구열 2기\n-아궁이, 주초와 적심, 저장용항아리, 건축물파편 폐기장이 포함\n-벽체의 잔존물 확인\n-철제유물로 볼 때 특수한 목적(대장간, 삼베가공)의 시설물로 추정", period: "1994", institution: "한양대학교 박물관", source: "한양대학교 박물관, 1995,\n『영송리 선사유적』", notes: "", pdf: "" },
        Row { num: "80", era: "조선", era_span: 0, date: "전기", site: "포천 야미리봉수\n영북면 야미리 봉화골 봉화뚝", artifacts: "병 구연부편, 도기편, 자기편 등", findings: "연조에 사용된 돌무지, 길이 20m 정도\n의 석축의 추정건물지 확인\n해발 165m\n북쪽 중군봉봉수, 남쪽 독현봉수와 응함", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사\n미로곡봉수", pdf: "" },
        Row { num: "81", era: "조선", era_span: 0, date: "전기", site: "포천 잉읍현봉수\n가산면 우금리 산86-1번지", artifacts: "회흑색 연질 토기 뚜껑,  연질토기편 등", findings: "평면 타원형 토축기단부의 봉수군 보\n호시설 조성, 5기의 석축연조 흔적\n-북쪽 독현봉수, 남쪽 대이산봉수 응함", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사", pdf: "" },
        Row { num: "82", era: "조선", era_span: 0, date: "전기", site: "포천 적골산 봉수지(중군봉 봉수지)\n영북면 자일리 중군봉", artifacts: "기와편", findings: "산 정상의 평탄면 확인되고 멸실 추정\n해발 250m\n조선보물고적조사자료 중군봉봉수지로 명칭\n북쪽 할미산봉수-남쪽 야미리봉수 연결", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사\n철원 경계", pdf: "" },
        Row { num: "83", era: "조선", era_span: 0, date: "전기", site: "포천 혜재곡 봉수지\n관인면 냉정2리 상냉동", artifacts: "기와편", findings: "현재는 인공구조물의 설치로 멸실\n-할미산 봉수의 추가로 기능이 약화", period: "1996", institution: "육군사관학교 육군박물관", source: "陸軍士官學校 陸軍博物館, 1997, 『京畿道 抱川郡 軍事遺蹟 地表調査 報告書』", notes: "지표조사", pdf: "" },
        Row { num: "84", era: "조선", era_span: 0, date: "전기", site: "금덕사(정불암)\n일동면 길명리 일원", artifacts: "", findings: "대웅전, 요사채\n-대웅전 정면 3칸, 측면 1칸, 팔작지붕\n-2단의 대지 조성", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "85", era: "조선", era_span: 0, date: "전기", site: "금주리 태봉\n영중면 금주리 480임 일원", artifacts: "", findings: "태실\n공사시 가마솥 형태의 석재가 뒤집어진채로 출토된 후 재매몰", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "86", era: "조선", era_span: 0, date: "전기", site: "무봉리 태봉\n소흘읍 무봉2리 일원", artifacts: "", findings: "영조 옹주아기씨 태실\n-원래 무봉리 태봉산에 위치하나 현재의 위치로 옮겨놓음. 원래의 위치 파악불가.", period: "2003~2004", institution: "단국대학교\n매장문화재연구소", source: "단국대학교 매장문화재연구소, 2004, 『文化遺蹟分布地圖-抱川市』", notes: "", pdf: "" },
        Row { num: "87", era: "조선", era_span: 0, date: "전기", site: "포천 중리 용수재울 유적\n관인면 중리 552번지 일원", artifacts: "청해파문·복합문·무문·어골문 암키와, 고려청자호, 조선청자 접시, 분청자편, 백자 접시·발·종자·(소)병, 도기흑유호·장군·(소)호·옹, 철정, 철촉, 철제고리, 도자편, 편자편, 철솥뚜껑, 청동숟가락, 청동합뚜껑, 석재약봉 등", findings: "건물지 3동, 주거지 2기, 수혈 1기\n-평면형태 'ㅡ', 'ㄱ' 로 추정\n-주거지 평면형태 말각방형에 부뚜막\n만 확인", period: "2010~2013", institution: "겨레문화유산연구원", source: "겨레문화유산연구원, 2016,\n『포천 중리 용수재울 유적』", notes: "", pdf: "" },
        Row { num: "88", era: "조선", era_span: 0, date: "전기", site: "포천 삼정리유적\n신북면 삼정리 521-1전 일원", artifacts: "백자, 도기장군, 흑갈유호, 청동호리병, 청동숟가락, 청동합, 구슬, 철제가위, 미상목기, 짚신, 과정 등", findings: "측구부탄요 1기, 회곽묘 5기, 토광묘 18기, 추정 석곽묘 1기\n-측구부탄요는 규모가 작아 가장 늦은 \n단계로 판단\n-석곽묘는 주변지형, 퇴적상태로 볼 때\n조선시대 이전으로는 불가 추정", period: "2011", institution: "국강고고학연구소", source: "국강고고학연구소, 2013,\n『포천 삼정리유적』", notes: "표본조사 보고서 미확보\n(경인)", pdf: "" },
        Row { num: "89", era: "조선", era_span: 0, date: "전기", site: "포천 대회산리 구석기유적\n영북면 대회산리 일원", artifacts: "출토유물 없음", findings: "토광묘 2기", period: "2011~2012", institution: "국방문화재연구원", source: "국방문화재연구원, 2015,\n『포천 대회산리 구석기유적』", notes: "", pdf: "" },
        Row { num: "90", era: "조선", era_span: 0, date: "전기", site: "포천 운산리 뒷골 구석기유적\n창수면 운산리 133-1번지 일원", artifacts: "암키와, 수키와 4점", findings: "기와가마 1기\n-반지하식 무단식 평요", period: "2011~2013", institution: "한국문화유산연구원", source: "韓國文化遺産硏究院, 2016,\n『抱川 雲山里 뒷골 舊石器遺蹟』", notes: "", pdf: "" },
        Row { num: "91", era: "조선", era_span: 0, date: "전기", site: "포천 용정리 유적(A구역)\n군내면 용정리·유교리 일원", artifacts: "청동경, 녹갈유 자기병 2점", findings: "조선시대 토광묘 2기, 회묘 1기\n미상 수혈주거지 1동", period: "2012~2013", institution: "한백문화재연구원", source: "한백문화재연구원, 2016,\n『포천 용정리 유적-포천 용정일반산업단지 조성사업A구역 내 유적 시굴 및 A-1지점 발굴조사 보고서』", notes: "", pdf: "" },
        Row { num: "92", era: "조선", era_span: 0, date: "전기", site: "포천 신평리 유적\n신북면 신평리 214-1번지 일원", artifacts: "백자 발, 청해파문·능형문 수키와편, 청해파문·사격자문·복합문·무문 암키와편 등 25점", findings: "기와가마 1기, 폐기유구 2기, 목탄요 1기\n-기와가마의 잔존상태는 불량하나 세장방형에 반지하식 등요 추정\n-목탄요 평면형태 역사다리꼴, 소성실,\n배연부 잔존", period: "2012~2013", institution: "국강고고학연구소", source: "국강고고학연구소, 2015,\n『포천 신평리유적』", notes: "", pdf: "" },
        Row { num: "93", era: "조선", era_span: 0, date: "전기", site: "포천 해룡산 건물지 유적\n설운동 산 38번지 일원", artifacts: "청해파문·종선문 수키와, 청해파문 암키와, 백제 대접, 발, 접시, 향완, 솥뚜껑 등", findings: "건물지 2동\n-1호 건물지 정면 2칸, 측면 1칸, 담장, \n3열의 고래열 연결 추정\n-2호 건물지 정면, 측면 2칸\n1열의 고래열 연결 추정\n-일반 민가 혹은 해룡산사지 혹은 안\n국사와 관련 가능성 존재", period: "2013", institution: "한백문화재연구원", source: "한백문화재연구원, 2015,\n『포천 해룡산 건물지 유적』", notes: "", pdf: "" },
        Row { num: "94", era: "조선", era_span: 0, date: "전기", site: "포천 참퀸스 컨트리클럽 진입로도 예정부지 내 유적(설운동 유물산포지2)\n설운동 산34-8번지 일원", artifacts: "", findings: "조선시대 추정주거지 1기, 석축1기, 시대미상 석곽고, 석축1기, 이장묘 1기\n-원형녹지보존지역으로 발굴유예", period: "2014", institution: "한백문화재연구원", source: "한백문화재연구원, 2015,\n『포천 참퀸스 컨트리클럽 진입도로 예정부지 내 유적 시굴조사 보고서』", notes: "기존정리자료 누락자료", pdf: "" },
        Row { num: "95", era: "조선", era_span: 0, date: "전기", site: "포천 소흘읍유적(초가팔리 유물산포지2)\n소흘읍 죽엽산로 56-58 일원", artifacts: "출토유물 없음", findings: "토광묘 1기\n-이단굴광 토광묘", period: "2014", institution: "국방문화재연구원", source: "국방문화재연구원, 2018,\n『포천 소흘읍 유적』", notes: "", pdf: "" },
        Row { num: "96", era: "조선", era_span: 0, date: "전기", site: "포천 소흘읍유적(초가팔리 유물산포지1)\n소흘읍 초가팔리 389번지 일원", artifacts: "분청자접시편, 백자편 등", findings: "회곽묘 2기, 토광묘 8기, 수혈 3기,\n구1기", period: "2014~2015", institution: "국방문화재연구원", source: "국방문화재연구원, 2018,\n『포천 소흘읍 유적』", notes: "", pdf: "" },
        Row { num: "97", era: "조선", era_span: 0, date: "전기", site: "포천 거사리 신석기·원삼국시대 주거지 유적\n영중면 거사리 370-10번지 일원", artifacts: "백자편", findings: "수혈유구 14기", period: "2015~2016", institution: "동국문화재연구원", source: "동국문화재연구원, 2018,\n『포천 거사리 신석기·원삼국시대 주거지』", notes: "", pdf: "" },
        Row { num: "98", era: "조선", era_span: 0, date: "전기", site: "포천 오가리·주원리 유적\n창수면 오가리, 주원리 일원", artifacts: "분청자편, 도기편", findings: "주거지 1기, 수혈유구 2기, 경작유구 1기\n-비교적 대규모 경작지 추정", period: "2015~2016", institution: "고려문화재연구원", source: "高麗文化財硏究院, 2018,\n『抱川 伍佳里·注院里 遺蹟』", notes: "", pdf: "" },
        Row { num: "99", era: "조선", era_span: 0, date: "전기", site: "포천 주원리유적\n창수면 주원리 681-2전 일원", artifacts: "분청자 호, 초벌구이 발편, 분청자 발·접시, 백자편, 도기 파수부편등", findings: "조선시대 주거지 1기", period: "2015~2016", institution: "한강문화재연구원", source: "한강문화재연구원, 2018,\n『포천 주원리 유적』", notes: "", pdf: "" },
        Row { num: "100", era: "조선", era_span: 0, date: "전기", site: "포천 독산봉수지\n신북면 기지리 590번지 일원", artifacts: "수키와, 도기편, 분청사기편, 백자접시·발, 갈유자기편, 청화백자 발, 석환 등", findings: "봉수대(방호벽, 연대, 연조 3기, 고사,\n망덕 등 확인)\n-포천시 향토유적 제51호\n-지표, 시굴조사만 진행\n-제1거 직봉(북쪽 영평 미로곡봉수,\n남쪽 포천 잉읍현봉수 연결)\n-전기부터 운영시작하여 1895년 이전 이미 기능을 상실\n-내지봉수 중 대규모에 해당", period: "2017~2018", institution: "한백문화재연구원", source: "한백문화재연구원, 2020,\n『포천 독산봉수지』", notes: "", pdf: "" },
        Row { num: "101", era: "조선", era_span: 0, date: "전기", site: "포천 구읍리 유적(1·2지점)\n군내면 구읍리 291-2번지, 산23-1번지 일원", artifacts: "명문 암키와, 명문수키와편, 단경호, 청자완편, 호 동체부편, 백자 발, 엽전, 동경, 무령, \n동전, 석제관옥 등", findings: "주거지 8기, 건물지 3기, 우물 1기, 수혈유구 40기, 토광묘 21기, 회곽묘 7기, 소성유구 4기", period: "2021", institution: "한국고고인류연구소", source: "한국고고인류연구소, 2021,\n『포천 구읍리 유적 1·2지점』", notes: "", pdf: "" },
        Row { num: "102", era: "조선", era_span: 0, date: "전기", site: "포천 구읍리 530-4번지 유적\n군내면 구읍리 530-4번지 일원", artifacts: "암막새, 수키와, 암키와, 청자 접시, 백자 발·접시·종자·잔, 도기 호·소호·연적, 상평통보, 철정, 편자, 석조나한상 등", findings: "건물지 3기, 계단 1기, 축대 6기, 석열 2기\n-향토유적 55호\n-조선시대 포천현 관아터\n-계획적인 대지조성 확인\n-1호 건물지가 가장 양호, 내부시설 축대, 기단, 초석(적심), 계단 온돌시설 등 확인\n-3칸 건물, 양쪽에 온돌방, 가운데 마루가 있는 구조\n-문지, 잡석지정, 초석, 온돌시설 등으로 볼 때 관아건물로 판단\n-용문암막새, 나한상 등으로 볼 때 관아 이전 사찰의 존재가능성", period: "2021", institution: "대한문화재연구원", source: "대한문화재연구원, 2023,\n『포천 구읍리 530-4번지 유적』", notes: "", pdf: "" },
        Row { num: "103", era: "조선", era_span: 0, date: "전기", site: "포천 설운동 479-2번지\n설운동 479-2번지 일원", artifacts: "출토유물 없음", findings: "탄요 1기, 수혈 6기", period: "2022~2023", institution: "백두문화연구원", source: "백두문화연구원, 2025,\n『포천 설운동 479-2번지 일원』", notes: "", pdf: "" },
        Row { num: "104", era: "조선", era_span: 0, date: "전기", site: "포천 감암리 187-6번지 유적\n가산면 감암리 187-6번지 일원", artifacts: "관정", findings: "토광묘 5기\n-같은 문중의 묘역 가능성 존재", period: "2023", institution: "국토문화유산연구원", source: "국토문화유산연구원, 2024,\n『포천 감암리 187-6번지 유적』", notes: "", pdf: "" },
        Row { num: "105", era: "조선", era_span: 0, date: "전기", site: "포천 음현리(224-1번지) 유적\n내촌면 음현리 224-1번지 일원", artifacts: "출토유물 없음", findings: "회곽묘 1기", period: "2023", institution: "경강문화유산연구원", source: "경강문화유산연구원, 2025,\n『抱川 陰峴里(224-1蕃地) 遺蹟』", notes: "", pdf: "" },
        Row { num: "106", era: "조선", era_span: 0, date: "전기", site: "포천 내리 352번지 유적\n내촌면 내리 352번지", artifacts: "상평통보", findings: "주거지 5동, 수혈유구 3기\n-축조양상이 유사하여 동일집단 추정\n-구들 흔적 확인", period: "2023", institution: "서울문화유산연구원", source: "서울문화유산연구원, 2025,\n『포천 내리 352번지 유적』", notes: "", pdf: "" },
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
    println!("=== 포천시 발굴연표 ===\n");
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
            "포천시 발굴연표".to_string()
        } else {
            format!("포천시 발굴연표 ({})", i + 1)
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
