# 실험 2 재설계 (v2) — Truncation 제거, 전체 문서 비교

> Status: 설계 완료 (2026-04-10)
> 이전 실험의 문제: 180K자 truncation으로 XML은 3.5%, MD는 75%만 전달 → 불공정 비교
> 해결: truncation 제거, 문서 전체를 AI에게 전달

## 1. 문제 인식

이전 실험(v1)에서 모든 입력을 180,000자에서 잘랐다. 이로 인해:

- XML: 원본의 3.5~26%만 AI에게 전달
- MD: 원본의 75~100% 전달
- MD가 더 잘 답하는 건 당연 → 불공정 비교

## 2. 재설계 원칙

1. **Truncation 제거**: 문서 전체를 AI에게 전달
2. **컨텍스트 초과 시**: "처리 불가"로 기록 (이것도 데이터)
3. **공정 비교**: 둘 다 전체를 읽은 상태에서 답변 품질 + 토큰 소비 비교

## 3. 모델 선택: Gemini 2.5 Flash (1M 토큰)

가장 큰 컨텍스트(1M 토큰)를 가진 모델로 XML 전체 처리 가능성 극대화.

## 4. 문서별 예상

| 문서          | XML 원본 크기 | XML 예상 토큰 | Gemini 처리 | MD 원본 크기 | MD 예상 토큰 | Gemini 처리 |
| ------------- | ------------- | ------------- | ----------- | ------------ | ------------ | ----------- |
| doc1 재난안전 | 1,221,516자   | ~500K         | ✅ 가능     | 61,195자     | ~55K         | ✅          |
| doc2 국무회의 | 1,156,865자   | ~470K         | ✅ 가능     | 79,891자     | ~72K         | ✅          |
| doc3 국정감사 | 5,111,680자   | ~2,000K       | ❌ 초과     | 239,484자    | ~177K        | ✅          |
| doc4 바이오   | 688,831자     | ~280K         | ✅ 가능     | 35,147자     | ~18K         | ✅          |
| doc5 계획서   | 5,646,067자   | ~2,300K       | ❌ 초과     | 165,218자    | ~70K         | ✅          |

## 5. 3단계 결과 구조

### 5.1 소형 문서 (doc4: XML ~280K tokens)

- XML 전체 ✅ + MD 전체 ✅ → **공정 비교**
- 비교: 답변 품질 동일한지? 토큰 소비 차이는?
- 예상: XML ~280K tokens vs MD ~18K tokens → **15배 비용 차이**

### 5.2 중형 문서 (doc1, doc2: XML ~470-500K tokens)

- Gemini에서 XML 전체 ✅ + MD 전체 ✅ → **공정 비교**
- 비교: 답변 품질 + 토큰 소비
- 예상: XML ~470-500K tokens vs MD ~55-72K tokens → **7-9배 비용 차이**

### 5.3 대형 문서 (doc3, doc5: XML ~2M+ tokens)

- XML 전체 ❌ (현존 어떤 AI도 처리 불가) + MD 전체 ✅
- **"DDI 없이는 처리 자체가 불가능"** — 가장 강력한 발견
- 참고: 이 문서들의 DDI MD도 70-177K tokens으로 모든 모델에서 처리 가능

## 6. 스크립트 수정 사항

`e2_run_experiment.py` 수정:

- `truncate_if_needed` 함수의 max_chars를 무제한으로 변경 (또는 함수 비활성화)
- API 컨텍스트 초과 에러 시 "CONTEXT_EXCEEDED" 상태로 기록
- Gemini 모델만 사용 (1M 컨텍스트)

```python
# 변경 전
def truncate_if_needed(text: str, max_chars: int = 180000):

# 변경 후
def truncate_if_needed(text: str, max_chars: int = 10_000_000):  # 사실상 무제한
```

## 7. 측정 지표

| 지표                 | 설명                               |
| -------------------- | ---------------------------------- |
| **처리 가능 여부**   | API가 요청을 수용했는가 (Y/N)      |
| **입력 토큰**        | 실제 소비된 input tokens           |
| **출력 토큰**        | 실제 소비된 output tokens          |
| **답변 정확도**      | gold answer 대비 정확성            |
| **응답 시간**        | elapsed_sec                        |
| **XML/MD 토큰 비율** | 같은 문서의 XML tokens / MD tokens |

## 8. 예상 논문 표

**Table: Full-Document Processing — XML vs DDI Markdown (Gemini 2.5 Flash, 1M context)**

| Document         | Size Category | XML Tokens | MD Tokens | Ratio | XML Processable | MD Processable | XML Accuracy | MD Accuracy |
| ---------------- | ------------- | ---------- | --------- | ----- | --------------- | -------------- | ------------ | ----------- |
| R&D Announcement | Small         | ~280K      | ~18K      | 15x   | ✅              | ✅             | ?/5          | ?/5         |
| Disaster Mgmt    | Medium        | ~500K      | ~55K      | 9x    | ✅              | ✅             | ?/5          | ?/5         |
| Cabinet Minutes  | Medium        | ~470K      | ~72K      | 7x    | ✅              | ✅             | ?/5          | ?/5         |
| National Audit   | Large         | ~2,000K    | ~177K     | 11x   | ❌              | ✅             | N/A          | ?/5         |
| R&D Proposal     | Large         | ~2,300K    | ~70K      | 33x   | ❌              | ✅             | N/A          | ?/5         |

## 9. 논문 서사

> "DDI Markdown은 XML 대비 문서의 의미 정보를 7-33배 적은 토큰으로 전달한다. 소형 문서에서는 동일한 답변 품질을 7-15배 적은 비용으로 달성하며, 대형 문서(5MB+)에서는 Raw XML이 현존하는 가장 큰 AI 컨텍스트(1M 토큰)를 초과하여 처리 자체가 불가능한 반면 DDI Markdown은 모든 모델에서 처리 가능하다."

## 10. 실행 순서

1. 스크립트에서 truncation 제거
2. Gemini로 5문서 전체 실행 (truncation 없이)
3. doc3, doc5의 XML에서 컨텍스트 초과 에러 기록
4. 나머지 3문서에서 공정 비교 (답변 품질 + 토큰)
5. 결과 정리 → 논문 표 작성
