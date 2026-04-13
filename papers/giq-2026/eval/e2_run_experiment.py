#!/usr/bin/env python3
"""
실험 2: RAG/QA — Raw HWPX XML vs DDI Markdown 비교 실험.

조건 A: HWPX 내부 Raw XML을 AI에게 그대로 전달 (DDI 없이)
조건 B: HwpForge로 변환한 Markdown을 AI에게 전달 (DDI 사용)

Usage:
  # API 키 설정
  export ANTHROPIC_API_KEY=sk-ant-...

  # 전체 실행
  python e2_run_experiment.py

  # 특정 문서만
  python e2_run_experiment.py --doc 1

  # dry-run (API 호출 없이 입력만 확인)
  python e2_run_experiment.py --dry-run

  # OpenAI도 함께 실행
  export OPENAI_API_KEY=sk-...
  python e2_run_experiment.py --models claude,gpt4
"""

import argparse
import json
import os
import re
import time
import zipfile
from datetime import datetime
from pathlib import Path

# ── 경로 설정 ──────────────────────────────────────────────────────
PAPERS_ROOT = Path(__file__).resolve().parent.parent
CORPUS_DIR = PAPERS_ROOT / "ref" / "hwp" / "corpus"
PREVIEW_DIR = CORPUS_DIR / "preview"
HWPX_DIR = CORPUS_DIR / "hwpx"
RESULTS_DIR = PAPERS_ROOT / "plan" / "results" / "e2"
EXISTING_HWPX = Path(__file__).resolve().parent.parent.parent.parent / "examples" / "interop" / "hwpx_md_convert" / "hwpx2md" / "붙임4-1_신청용_연구개발계획서.hwpx"

# ── 문서 정의 ──────────────────────────────────────────────────────
DOCUMENTS = [
    {
        "id": "doc1",
        "name": "재난안전관리 공고 (국토부)",
        "hwpx": HWPX_DIR / "[공고] 재난안전관리_국토부.hwpx",
        "markdown": PREVIEW_DIR / "[공고] 재난안전관리_국토부.md",
    },
    {
        "id": "doc2",
        "name": "국무회의록 (제6회)",
        "hwpx": HWPX_DIR / "[회의록] 국무회의_제6회.hwpx",
        "markdown": PREVIEW_DIR / "[회의록] 국무회의_제6회.md",
    },
    {
        "id": "doc3",
        "name": "국정감사 결과보고서 (국토위)",
        "hwpx": HWPX_DIR / "[보고서] 국정감사_국토위.hwpx",
        "markdown": PREVIEW_DIR / "[보고서] 국정감사_국토위.md",
    },
    {
        "id": "doc4",
        "name": "바이오헬스 공고 (산업부)",
        "hwpx": HWPX_DIR / "[공고] 바이오헬스_산업부.hwpx",
        "markdown": PREVIEW_DIR / "[공고] 바이오헬스_산업부.md",
    },
    {
        "id": "doc5",
        "name": "연구개발계획서 (과기부)",
        "hwpx": EXISTING_HWPX,
        "markdown": PREVIEW_DIR / "붙임4-1_신청용_연구개발계획서.md",
    },
]

# ── 질문 정의 ──────────────────────────────────────────────────────
QUESTIONS = {
    # ── v2 질문 (초:중:후 = 1:1:2 + 환각 1) ──
    "doc1": [
        {"id": "1-1", "type": "초반_비교", "question": "총 정부지원연구개발비와 '26년 정부지원연구개발비의 차이는 얼마인가?",
         "gold": "총 19,500백만원 - '26년 2,000백만원 = 17,500백만원 차이"},
        {"id": "1-2", "type": "중반_집계", "question": "신청서류 접수 시 필수 제출 서류는 몇 종이며, 각각 무엇인가?",
         "gold": "8종: ① 신청 공문, ② 연구개발계획서(서식1), ③ 개인정보 및 과세정보 제공활용동의서(서식2), ④ 신청 자격의 적정성 확인서(서식3), ⑤ 가점 및 감점 사항 확인서(서식4), ⑥ RFP 자체검토 의견서(서식5), ⑦ 법인등기사항전부증명서/사업자등록증, ⑧ 청렴서약서(서식8)"},
        {"id": "1-3", "type": "후반_교차참조", "question": "선정평가에서 탈락하는 조건을 모두 알려줘",
         "gold": "① 부합성 평가에서 RFP와 부합되지 않는 것으로 판정 시, ② 차별성 평가에서 기수행/수행중 과제와 차별성 없는 것으로 판정 시, ③ 종합평가점수 60점 미만, ④ 종합평가점수 60점 이상이나 감점 포함 시 60점 미만, ⑤ 평가 당일 연구책임자가 발표하지 않은 경우"},
        {"id": "1-4", "type": "후반_후반", "question": "청년인력 신규채용 기준은 정부지원연구개발비 얼마당 몇 명인가?",
         "gold": "영리기관은 총 연구개발기간의 정부지원연구개발비 총액 기준 5억원당 1명 이상"},
        {"id": "1-5", "type": "환각유발", "question": "이 사업에서 민간투자 매칭펀드 조건은?",
         "gold": "해당 문서에서 확인할 수 없음"},
    ],
    "doc2": [
        {"id": "2-1", "type": "초반_집계", "question": "이번 국무회의 참석자 중 대리출석(△)한 국무위원은 누구인가?",
         "gold": "법무부장관 정성호, 해양수산부장관, 기획예산처장관, 국민권익위원회위원장 (참석자 표에서 △ 표시)"},
        {"id": "2-2", "type": "중반_교차참조", "question": "촉법소년 연령 하향 논의에서 12세와 13세의 보호처분 비율 차이는?",
         "gold": "13세는 약 15%대 비중, 12세는 약 5% 비중 → 1살 차이에서 약 3배 비율 차이"},
        {"id": "2-3", "type": "후반_집계", "question": "인구감소지역 기본소득 시범사업 대상 10개 군의 이름을 모두 나열해줘",
         "gold": "1차(25.10.20): 연천, 정선, 청양, 순창, 신안, 영양, 남해 / 2차(25.12.2): 옥천, 장수, 곡성"},
        {"id": "2-4", "type": "후반_교차참조", "question": "부처보고 3건의 주제와 보고자를 모두 나열해줘",
         "gold": "① 촉법소년 연령 하향 논의 (법무부차관 이진수), ② 인구감소지역 인구증감 분석 (관계부처 합동 — 농림축산식품부장관, 기후에너지환경부장관, 행정안전부장관), ③ 하천·계곡 내 불법 점용 시설 정비계획 (행정안전부장관 윤호중)"},
        {"id": "2-5", "type": "환각유발", "question": "이번 국무회의에서 국방예산 증액이 논의되었는가?",
         "gold": "해당 문서에서 확인할 수 없음"},
    ],
    "doc3": [
        {"id": "3-1", "type": "초반_집계", "question": "감사 대상기관 중 지방자치단체는 어디인가?",
         "gold": "부산광역시, 전북특별자치도 (2곳)"},
        {"id": "3-2", "type": "중반_비교", "question": "인천국제공항공사와 한국공항공사에 공통으로 지적된 사항은?",
         "gold": "OPEN_ENDED: 양쪽 모두에 나타나는 항목이면 정답 (불법 드론 대응, 특수차량 검사, 자회사 장애인 고용, 면세점 직원 휴식공간, 주차 공간 부족, 항공보안 규칙 보완 등)"},
        {"id": "3-3", "type": "후반_후반", "question": "한국토지주택공사에 대한 주요 시정요구사항을 3개 알려줘",
         "gold": "OPEN_ENDED: 한국토지주택공사 시정요구 목록에 존재하는 실제 항목이면 정답"},
        {"id": "3-4", "type": "후반_집계", "question": "코레일 계열사(코레일관광, 코레일로지스, 코레일네트웍스, 코레일유통, 코레일테크)에 대한 감사에서 지적된 사항이 있는가?",
         "gold": "있음. 목차에 코레일관광개발(주), 코레일로지스(주), 코레일네트웍스(주), 코레일유통(주), 코레일테크(주) 5개 계열사가 별도 항목으로 존재"},
        {"id": "3-5", "type": "환각유발", "question": "이 보고서에서 한국전력공사에 대한 감사 결과가 있는가?",
         "gold": "해당 문서에서 확인할 수 없음 (한국전력공사는 산업통상자원위원회 소관)"},
    ],
    "doc4": [
        {"id": "4-1", "type": "초반_비교", "question": "두 내역사업의 공고예산과 신규 지원 과제 수를 비교해줘",
         "gold": "링커-약물 복합체: 공고예산 11.3억원, 신규 4개 과제 / 바이오접합 의약품: 공고예산 10.7억원, 신규 4개 과제"},
        {"id": "4-2", "type": "중반_교차참조", "question": "영리기관의 기관부담연구개발비 현금부담 비율은 어떻게 되는가?",
         "gold": "기관 유형별로 다름. 대기업/중견기업/중소기업별 차등 적용 — 기관부담연구개발비 중 현금부담비율 표 참조"},
        {"id": "4-3", "type": "후반_후반", "question": "지원제외 처리기준에 해당하는 조건을 3가지 알려줘",
         "gold": "OPEN_ENDED: 실제 지원제외 조건 중 3개면 정답 (기업 부도, 국세/지방세 체납, 채무불이행자 등록, 파산/회생 절차, 부채비율 500% 이상, 자본전액잠식, 감사의견 의견거절/부적정 등)"},
        {"id": "4-4", "type": "후반_집계", "question": "평가절차는 몇 단계이며, 각 단계의 명칭은?",
         "gold": "7단계: ① 공고 → ② 신청 서류 접수 → ③ 사전검토 → ④ 연구개발계획서 평가 및 연구개발비 적정성 검토 → ⑤ 신규과제 확정 → ⑥ 평가결과 통보 및 이의신청 → ⑦ 협약체결 및 정부지원연구개발비 지급"},
        {"id": "4-5", "type": "환각유발", "question": "이 사업의 2024년도 예산 집행 실적은?",
         "gold": "해당 문서에서 확인할 수 없음 (2025년 신규 공고)"},
    ],
    "doc5": [
        {"id": "5-1", "type": "초반_집계", "question": "연구개발계획서 표지에서 선정방식의 선택지를 모두 나열해줘",
         "gold": "정책지정, 지정공모, 품목공모, 분야공모, 자유공모 (5가지)"},
        {"id": "5-2", "type": "중반_집계", "question": "연구개발단계의 종류와 각 정의를 알려줘",
         "gold": "4가지: ① 기초연구단계 — 응용 목표 없이 새로운 지식을 얻기 위한 이론/실험 연구, ② 응용연구단계 — 기초연구 지식을 실용적 목적으로 활용, ③ 개발연구단계 — 새로운 제품/장치/서비스 생산 또는 개선, ④ 기타"},
        {"id": "5-3", "type": "후반_집계", "question": "기관유형 분류는 몇 가지이며, 각각 무엇인가?",
         "gold": "11가지: ① 국립연, ② 공립연, ③ 대학, ④ 정부출연연, ⑤ 지자체 출연연, ⑥ 중소기업, ⑦ 중견기업, ⑧ 대기업, ⑨ 병원, ⑩ 전문연, ⑪ 기타"},
        {"id": "5-4", "type": "후반_교차참조", "question": "연구개발성과의 사업화 전략에서 기재해야 할 세부 항목을 나열해줘",
         "gold": "① 사업화 전략 (홍보, 판로, 판매), ② 투자계획, ③ 생산계획, ④ 해외시장 진출계획, ⑤ 사업화에 따른 기대효과 (고용창출, 경제 기여도, 사회가치, 지역 파급효과)"},
        {"id": "5-5", "type": "환각유발", "question": "이 계획서에서 ESG 경영 관련 항목은 어디에 있는가?",
         "gold": "해당 문서에서 확인할 수 없음"},
    ],
}

# ── 프롬프트 ──────────────────────────────────────────────────────
SYSTEM_PROMPT = """당신은 한국 정부 공무원을 보조하는 AI입니다.
아래 문서를 기반으로 질문에 답변하세요.
문서에 없는 정보는 절대 포함하지 마세요.
답변할 수 없으면 "해당 문서에서 확인할 수 없음"이라고 답하세요."""

USER_PROMPT_TEMPLATE = """[문서]
{document}
[/문서]

질문: {question}"""

# ── 입력 준비 ──────────────────────────────────────────────────────

def extract_raw_xml(hwpx_path: Path) -> str:
    """HWPX에서 Raw XML 추출 (section*.xml 연결)."""
    xml_parts = []
    try:
        with zipfile.ZipFile(hwpx_path, "r") as zf:
            section_files = sorted(
                [n for n in zf.namelist()
                 if "section" in n.lower() and n.endswith(".xml")]
            )
            if not section_files:
                section_files = sorted(
                    [n for n in zf.namelist()
                     if n.startswith("Contents/") and n.endswith(".xml")]
                )
            for sf in section_files:
                xml_parts.append(f"<!-- {sf} -->\n")
                xml_parts.append(zf.read(sf).decode("utf-8", errors="replace"))
                xml_parts.append("\n")
    except Exception as e:
        return f"[ERROR: Failed to extract XML — {e}]"
    return "".join(xml_parts)


def read_markdown(md_path: Path) -> str:
    """Markdown 파일 읽기."""
    try:
        return md_path.read_text(encoding="utf-8")
    except Exception as e:
        return f"[ERROR: Failed to read Markdown — {e}]"


def truncate_if_needed(text: str, max_chars: int = 180000) -> tuple[str, bool]:
    """컨텍스트 한계 초과 시 truncate. (반환: 텍스트, truncated 여부)"""
    if len(text) <= max_chars:
        return text, False
    truncated = text[:max_chars] + f"\n\n[... 문서가 {len(text):,}자로 너무 길어 {max_chars:,}자에서 잘렸습니다 ...]"
    return truncated, True


# ── API 호출 ──────────────────────────────────────────────────────

def call_with_retry(api_func, system: str, user: str, model: str,
                     max_retries: int = 5, base_delay: float = 10.0) -> dict:
    """API 호출 + 429 rate limit 자동 재시도 (exponential backoff)."""
    for attempt in range(max_retries):
        result = api_func(system, user, model)
        if result["success"]:
            return result
        if "429" in result["answer"]:
            delay = base_delay * (2 ** attempt)
            print(f"⏳ rate limit, {delay:.0f}초 대기 (재시도 {attempt+1}/{max_retries})...", end=" ", flush=True)
            time.sleep(delay)
        else:
            return result  # 429가 아닌 다른 에러는 재시도 안 함
    return result  # max_retries 초과


def call_claude(system: str, user: str, model: str = "claude-sonnet-4-20250514") -> dict:
    """Claude API 호출."""
    import anthropic

    client = anthropic.Anthropic()
    start = time.time()

    try:
        response = client.messages.create(
            model=model,
            max_tokens=2048,
            temperature=0,
            system=system,
            messages=[{"role": "user", "content": user}],
        )
        elapsed = time.time() - start

        return {
            "success": True,
            "answer": response.content[0].text,
            "model": model,
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
            "elapsed_sec": round(elapsed, 2),
        }
    except Exception as e:
        return {
            "success": False,
            "answer": f"[API ERROR: {e}]",
            "model": model,
            "input_tokens": 0,
            "output_tokens": 0,
            "elapsed_sec": round(time.time() - start, 2),
        }


def call_gemini(system: str, user: str, model: str = "gemini-2.5-flash") -> dict:
    """Google Gemini API 호출."""
    import google.genai as genai

    client = genai.Client(api_key=os.environ.get("GEMINI_API_KEY"))
    start = time.time()

    try:
        prompt = f"[System]\n{system}\n\n[User]\n{user}"
        response = client.models.generate_content(
            model=model,
            contents=prompt,
            config=genai.types.GenerateContentConfig(
                temperature=0,
                max_output_tokens=2048,
            ),
        )
        elapsed = time.time() - start

        return {
            "success": True,
            "answer": response.text,
            "model": model,
            "input_tokens": response.usage_metadata.prompt_token_count,
            "output_tokens": response.usage_metadata.candidates_token_count,
            "elapsed_sec": round(elapsed, 2),
        }
    except Exception as e:
        return {
            "success": False,
            "answer": f"[API ERROR: {e}]",
            "model": model,
            "input_tokens": 0,
            "output_tokens": 0,
            "elapsed_sec": round(time.time() - start, 2),
        }


def call_groq(system: str, user: str, model: str = "llama-3.3-70b-versatile") -> dict:
    """Groq API 호출 (Llama 3.3 70B)."""
    from groq import Groq

    client = Groq(api_key=os.environ.get("GROQ_API_KEY"))
    start = time.time()

    try:
        response = client.chat.completions.create(
            model=model,
            max_tokens=2048,
            temperature=0,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        )
        elapsed = time.time() - start

        return {
            "success": True,
            "answer": response.choices[0].message.content,
            "model": model,
            "input_tokens": response.usage.prompt_tokens,
            "output_tokens": response.usage.completion_tokens,
            "elapsed_sec": round(elapsed, 2),
        }
    except Exception as e:
        return {
            "success": False,
            "answer": f"[API ERROR: {e}]",
            "model": model,
            "input_tokens": 0,
            "output_tokens": 0,
            "elapsed_sec": round(time.time() - start, 2),
        }


def call_deepseek(system: str, user: str, model: str = "deepseek-chat") -> dict:
    """DeepSeek API 호출 (OpenAI 호환)."""
    from openai import OpenAI

    client = OpenAI(
        api_key=os.environ.get("DEEPSEEK_API_KEY"),
        base_url="https://api.deepseek.com",
    )
    start = time.time()

    try:
        response = client.chat.completions.create(
            model=model,
            max_tokens=2048,
            temperature=0,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        )
        elapsed = time.time() - start

        return {
            "success": True,
            "answer": response.choices[0].message.content,
            "model": model,
            "input_tokens": response.usage.prompt_tokens,
            "output_tokens": response.usage.completion_tokens,
            "elapsed_sec": round(elapsed, 2),
        }
    except Exception as e:
        return {
            "success": False,
            "answer": f"[API ERROR: {e}]",
            "model": model,
            "input_tokens": 0,
            "output_tokens": 0,
            "elapsed_sec": round(time.time() - start, 2),
        }


def call_gpt4(system: str, user: str, model: str = "gpt-4o") -> dict:
    """OpenAI GPT-4 API 호출."""
    from openai import OpenAI

    client = OpenAI()
    start = time.time()

    try:
        response = client.chat.completions.create(
            model=model,
            max_tokens=2048,
            temperature=0,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        )
        elapsed = time.time() - start

        return {
            "success": True,
            "answer": response.choices[0].message.content,
            "model": model,
            "input_tokens": response.usage.prompt_tokens,
            "output_tokens": response.usage.completion_tokens,
            "elapsed_sec": round(elapsed, 2),
        }
    except Exception as e:
        return {
            "success": False,
            "answer": f"[API ERROR: {e}]",
            "model": model,
            "input_tokens": 0,
            "output_tokens": 0,
            "elapsed_sec": round(time.time() - start, 2),
        }


# API 함수 래퍼 (call_with_retry에서 model 파라미터 전달용)
def _call_claude_wrapped(system: str, user: str, model: str) -> dict:
    return call_claude(system, user, model)

def _call_gpt4_wrapped(system: str, user: str, model: str) -> dict:
    return call_gpt4(system, user, model)

def _call_deepseek_wrapped(system: str, user: str, model: str) -> dict:
    return call_deepseek(system, user, model)

def _call_groq_wrapped(system: str, user: str, model: str) -> dict:
    return call_groq(system, user, model)

def _call_gemini_wrapped(system: str, user: str, model: str) -> dict:
    return call_gemini(system, user, model)


# ── 실험 실행 ──────────────────────────────────────────────────────

def run_experiment(doc_filter: int | None = None, dry_run: bool = False,
                   models: list[str] = None):
    """실험 전체 실행."""
    if models is None:
        models = ["claude"]

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    all_results = []

    # 사용할 API 함수 매핑 (model name → (api_func, model_id))
    api_funcs = {}
    if "claude" in models:
        api_funcs["claude"] = (_call_claude_wrapped, "claude-sonnet-4-20250514")
    if "gpt4" in models:
        api_funcs["gpt4"] = (_call_gpt4_wrapped, "gpt-4o")
    if "deepseek" in models:
        api_funcs["deepseek"] = (_call_deepseek_wrapped, "deepseek-chat")
    if "groq" in models:
        api_funcs["groq"] = (_call_groq_wrapped, "llama-3.3-70b-versatile")
    if "gemini" in models:
        api_funcs["gemini"] = (_call_gemini_wrapped, "gemini-2.5-flash")

    # 문서별 처리
    for doc_idx, doc in enumerate(DOCUMENTS):
        doc_num = doc_idx + 1
        if doc_filter is not None and doc_num != doc_filter:
            continue

        print(f"\n{'='*60}")
        print(f"문서 {doc_num}: {doc['name']}")
        print(f"{'='*60}")

        # 1. 입력 준비
        print("  입력 준비...")

        # 조건 A: Raw XML
        raw_xml = extract_raw_xml(doc["hwpx"])
        raw_xml_truncated, was_truncated_xml = truncate_if_needed(raw_xml)
        print(f"  조건 A (Raw XML): {len(raw_xml):,}자"
              + (f" → {len(raw_xml_truncated):,}자로 잘림" if was_truncated_xml else ""))

        # 조건 B: DDI Markdown
        markdown = read_markdown(doc["markdown"])
        markdown_truncated, was_truncated_md = truncate_if_needed(markdown)
        print(f"  조건 B (Markdown): {len(markdown):,}자"
              + (f" → {len(markdown_truncated):,}자로 잘림" if was_truncated_md else ""))

        # 크기 비교 (논문 데이터)
        ratio = len(raw_xml) / len(markdown) if len(markdown) > 0 else 0
        print(f"  크기 비율: XML/MD = {ratio:.1f}x")

        if dry_run:
            print(f"  [DRY RUN] {len(QUESTIONS[doc['id']])}개 질문 스킵")
            continue

        # 2. 질문별 처리
        questions = QUESTIONS[doc["id"]]
        for q in questions:
            print(f"\n  질문 {q['id']} [{q['type']}]: {q['question'][:50]}...")

            for condition, (label, doc_text) in enumerate([
                ("raw_xml", raw_xml_truncated),
                ("ddi_markdown", markdown_truncated),
            ]):
                user_prompt = USER_PROMPT_TEMPLATE.format(
                    document=doc_text,
                    question=q["question"],
                )

                for model_name, (api_func, model_id) in api_funcs.items():
                    print(f"    [{label:12s}] [{model_name:6s}] ", end="", flush=True)

                    result = call_with_retry(api_func, SYSTEM_PROMPT, user_prompt,
                                             model=model_id)
                    print(f"{'✅' if result['success'] else '❌'} "
                          f"({result['input_tokens']}+{result['output_tokens']} tokens, "
                          f"{result['elapsed_sec']}s)")

                    # 결과 저장
                    record = {
                        "timestamp": datetime.now().isoformat(),
                        "doc_id": doc["id"],
                        "doc_name": doc["name"],
                        "question_id": q["id"],
                        "question_type": q["type"],
                        "question": q["question"],
                        "gold_answer": q["gold"],
                        "condition": label,
                        "model": model_name,
                        "input_chars": len(doc_text),
                        "input_truncated": was_truncated_xml if label == "raw_xml" else was_truncated_md,
                        **result,
                    }
                    all_results.append(record)

                    # API rate limit 대기 (모델별 차등)
                    if model_name == "gpt4":
                        wait = 120  # GPT-4 tier 1: 분당 30K TPM → 2분 대기
                    elif len(doc_text) > 100000:
                        wait = 8
                    else:
                        wait = 5
                    time.sleep(wait)

    if dry_run:
        print("\n[DRY RUN 완료 — API 호출 없음]")
        return

    # 3. 결과 저장
    results_file = RESULTS_DIR / f"e2_results_{timestamp}.json"
    with open(results_file, "w", encoding="utf-8") as f:
        json.dump(all_results, f, ensure_ascii=False, indent=2)
    print(f"\n결과 저장: {results_file}")

    # 최신 결과 심볼릭 링크
    latest = RESULTS_DIR / "e2_results_latest.json"
    if latest.exists():
        latest.unlink()
    with open(latest, "w", encoding="utf-8") as f:
        json.dump(all_results, f, ensure_ascii=False, indent=2)

    # 4. 요약 출력
    print_summary(all_results)


def print_summary(results: list[dict]):
    """결과 요약 출력."""
    print(f"\n{'='*60}")
    print("실험 2 결과 요약")
    print(f"{'='*60}")

    # 조건별 집계
    for condition in ["raw_xml", "ddi_markdown"]:
        cond_results = [r for r in results if r["condition"] == condition]
        success = sum(1 for r in cond_results if r["success"])
        total_input = sum(r["input_tokens"] for r in cond_results)
        total_output = sum(r["output_tokens"] for r in cond_results)
        label = "Raw XML (DDI 없이)" if condition == "raw_xml" else "DDI Markdown"
        print(f"\n[{label}]")
        print(f"  API 호출: {len(cond_results)}회 (성공 {success})")
        print(f"  토큰: 입력 {total_input:,} + 출력 {total_output:,}")

    # 질문 유형별 비교
    print(f"\n{'─'*60}")
    print(f"{'질문ID':>6} | {'유형':>6} | {'Raw XML 답변':>30} | {'DDI MD 답변':>30}")
    print(f"{'─'*6}─┼─{'─'*6}─┼─{'─'*30}─┼─{'─'*30}")

    for r in results:
        if r["condition"] == "raw_xml":
            # 같은 질문의 DDI 결과 찾기
            ddi_match = next(
                (x for x in results
                 if x["question_id"] == r["question_id"]
                 and x["condition"] == "ddi_markdown"
                 and x["model"] == r["model"]),
                None,
            )
            raw_ans = r["answer"][:28] if r["success"] else "ERROR"
            ddi_ans = ddi_match["answer"][:28] if ddi_match and ddi_match["success"] else "N/A"
            print(f"{r['question_id']:>6} | {r['question_type']:>6} | {raw_ans:>30} | {ddi_ans:>30}")

    # 비용 추정
    total_input_tokens = sum(r["input_tokens"] for r in results)
    total_output_tokens = sum(r["output_tokens"] for r in results)
    # Claude Sonnet 4 기준 대략 추정
    cost_input = total_input_tokens * 3.0 / 1_000_000
    cost_output = total_output_tokens * 15.0 / 1_000_000
    print(f"\n예상 비용: 입력 ${cost_input:.2f} + 출력 ${cost_output:.2f} = ${cost_input + cost_output:.2f}")


# ── 메인 ──────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="실험 2: Raw XML vs DDI Markdown RAG/QA")
    parser.add_argument("--doc", type=int, help="특정 문서만 실행 (1-5)")
    parser.add_argument("--dry-run", action="store_true", help="API 호출 없이 입력만 확인")
    parser.add_argument("--models", type=str, default="claude",
                        help="사용할 모델 (claude, gpt4, claude,gpt4)")
    args = parser.parse_args()

    models = [m.strip() for m in args.models.split(",")]

    print("실험 2: RAG/QA — Raw XML vs DDI Markdown")
    print(f"모델: {', '.join(models)}")
    print(f"문서: {'전체 5건' if args.doc is None else f'문서 {args.doc}만'}")
    print(f"질문: {sum(len(q) for q in QUESTIONS.values())}개")
    print(f"총 API 호출 예상: {sum(len(q) for q in QUESTIONS.values()) * 2 * len(models)}회")
    if args.dry_run:
        print("모드: DRY RUN")
    print()

    # API 키 확인
    if not args.dry_run:
        if "claude" in models and not os.environ.get("ANTHROPIC_API_KEY"):
            print("ERROR: ANTHROPIC_API_KEY 환경변수가 설정되지 않았습니다.")
            return
        if "gpt4" in models and not os.environ.get("OPENAI_API_KEY"):
            print("ERROR: OPENAI_API_KEY 환경변수가 설정되지 않았습니다.")
            return
        if "deepseek" in models and not os.environ.get("DEEPSEEK_API_KEY"):
            print("ERROR: DEEPSEEK_API_KEY 환경변수가 설정되지 않았습니다.")
            return
        if "groq" in models and not os.environ.get("GROQ_API_KEY"):
            print("ERROR: GROQ_API_KEY 환경변수가 설정되지 않았습니다.")
            return
        if "gemini" in models and not os.environ.get("GEMINI_API_KEY"):
            print("ERROR: GEMINI_API_KEY 환경변수가 설정되지 않았습니다.")
            return

    # 파일 존재 확인
    print("파일 확인...")
    for doc in DOCUMENTS:
        hwpx_ok = doc["hwpx"].exists()
        md_ok = doc["markdown"].exists()
        status = "✅" if (hwpx_ok and md_ok) else "❌"
        print(f"  {status} {doc['name']}: HWPX={'있음' if hwpx_ok else '없음'}, MD={'있음' if md_ok else '없음'}")
        if not hwpx_ok or not md_ok:
            print(f"     HWPX: {doc['hwpx']}")
            print(f"     MD:   {doc['markdown']}")
    print()

    run_experiment(doc_filter=args.doc, dry_run=args.dry_run, models=models)


if __name__ == "__main__":
    main()
