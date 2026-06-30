#!/usr/bin/env bash
# 노벨피아 비공식 API - curl 통합 테스트
#
# 사용법:
#   chmod +x tests/api_test.sh
#   ./tests/api_test.sh                    # 공개 엔드포인트만 테스트
#   LOGINKEY=xxx_yyy ./tests/api_test.sh   # 인증 포함 전체 테스트
#   NOVEL_NO=23 EP_CODE=1134 ./tests/api_test.sh  # 대상 지정
#
# 환경변수:
#   LOGINKEY     - LOGINKEY 쿠키 값 (<32hex>_<32hex>)
#   CSRF_TOKEN   - CSRF 토큰 (novel_like, board_option, member_block에 필요)
#   NOVEL_NO      - 테스트 소설 번호 (기본값: 23)
#   EPISODE_LIST_NOVEL_NO - 회차 목록 테스트용 소설 번호, 회차 보유 소설이어야 함 (기본값: 31631)
#   EP_CODE       - 무료 회차 번호 (기본값: 1134)
#   PAID_EP_CODE  - 유료 회차 번호 (기본값: 33296)
#   MEMBER_NO    - 차단 테스트용 회원 번호 (member_block 전용)
#   PROFILE_MEM_NO - 프로필 조회 테스트용 회원 번호 (get_member2 등, 기본값: 4169856)
#   UNBLOCKED_MEM_NO - 비차단 확인용 회원 번호 (get_user_block_chk, 기본값: 17)
#   VERBOSE      - 1로 설정 시 응답 본문 출력
#   MAX_JOBS     - 공개 GET 테스트(섹션 1·2)의 최대 동시 실행 수 (기본값: 6, 1이면 순차)

set -uo pipefail

BASE_URL="https://novelpia.com"
NOVEL_NO="${NOVEL_NO:-23}"
EPISODE_LIST_NOVEL_NO="${EPISODE_LIST_NOVEL_NO:-31631}"
EP_CODE="${EP_CODE:-1134}"
PAID_EP_CODE="${PAID_EP_CODE:-33296}"
MEMBER_NO="${MEMBER_NO:-}"
PROFILE_MEM_NO="${PROFILE_MEM_NO:-4169856}"
UNBLOCKED_MEM_NO="${UNBLOCKED_MEM_NO:-17}"
VERBOSE="${VERBOSE:-0}"
MAX_JOBS="${MAX_JOBS:-6}"

PASS=0
FAIL=0
SKIP=0

# ── 출력 헬퍼 ─────────────────────────────────────────────────
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
RESET='\033[0m'

pass() { echo -e "${GREEN}[PASS]${RESET} $1"; (( PASS++ )) || true; }
fail() { echo -e "${RED}[FAIL]${RESET} $1"; (( FAIL++ )) || true; }
skip() { echo -e "${YELLOW}[SKIP]${RESET} $1"; (( SKIP++ )) || true; }
info() { echo "       $1"; }

# ── 병렬 실행 인프라 ───────────────────────────────────────────
# 워커(서브셸)에서는 전역 카운터 증가가 부모로 전파되지 않으므로,
# 워커는 순수 출력만 하고 부모가 결과 파일의 [PASS]/[FAIL]/[SKIP] 라인을
# 세어 집계한다. wpass/wfail/wskip은 카운터 없는 출력 전용 변형이다.
wpass() { echo -e "${GREEN}[PASS]${RESET} $1"; }
wfail() { echo -e "${RED}[FAIL]${RESET} $1"; }
wskip() { echo -e "${YELLOW}[SKIP]${RESET} $1"; }

PAR_DIR=""            # 현재 병렬 배치의 결과 디렉터리
PAR_SEQ=0             # 배치 내 작업 순번 (출력 순서 보존용)

# 병렬 배치 시작: 결과 디렉터리 초기화
par_begin() {
    PAR_DIR=$(mktemp -d)
    PAR_SEQ=0
}

# 작업 제출: par_run <함수명> [인수...]
# 함수의 stdout 전체를 순번 파일에 기록한다. MAX_JOBS 초과 시 슬롯이 빌 때까지 대기.
par_run() {
    local seq
    seq=$(printf "%04d" "$PAR_SEQ")
    (( PAR_SEQ++ ))
    # 실행 중인 백그라운드 작업이 MAX_JOBS 이상이면 하나 끝날 때까지 대기
    while (( $(jobs -rp | wc -l) >= MAX_JOBS )); do
        wait -n 2>/dev/null || true
    done
    local outfile="${PAR_DIR}/${seq}.out"
    ( "$@" >"$outfile" 2>&1 ) &
}

# 배치 종료: 모든 작업 완료 대기 → 순번대로 출력 + PASS/FAIL/SKIP 집계
par_end() {
    wait
    local f
    for f in "$PAR_DIR"/*.out; do
        [[ -e "$f" ]] || continue
        cat "$f"
        local p fl s
        p=$(grep -c '\[PASS\]' "$f" || true)
        fl=$(grep -c '\[FAIL\]' "$f" || true)
        s=$(grep -c '\[SKIP\]' "$f" || true)
        (( PASS += p )) || true
        (( FAIL += fl )) || true
        (( SKIP += s )) || true
    done
    rm -rf "$PAR_DIR"
    PAR_DIR=""
}

# ── curl 래퍼 ─────────────────────────────────────────────────
# 인수: <설명> <기대_상태코드> [curl 옵션...]
# 반환: 응답 본문을 $BODY에 저장, HTTP 상태를 $STATUS에 저장
do_request() {
    local desc="$1"
    local expected_status="$2"
    shift 2

    local tmp
    tmp=$(mktemp)

    STATUS=$(curl -s -o "$tmp" -w "%{http_code}" \
        -H "User-Agent: novelpia-api-test/0.1 (unofficial)" \
        "$@") || { fail "$desc (curl 오류)"; rm -f "$tmp"; return 1; }

    BODY=$(cat "$tmp")
    rm -f "$tmp"

    if [[ "$STATUS" == "$expected_status" ]]; then
        pass "$desc [HTTP $STATUS]"
        if [[ "$VERBOSE" == "1" ]]; then
            info "응답: ${BODY:0:200}"
        fi
    else
        fail "$desc [HTTP $STATUS, 기대: $expected_status]"
        info "응답: ${BODY:0:200}"
    fi
}

# GET 200 단축 헬퍼: get_page <경로> <레이블> [추가 curl 옵션...]
get_page() {
    local path="$1"
    local label="$2"
    shift 2
    do_request "GET ${path} (${label})" "200" "$@" "${BASE_URL}${path}"
}

# ── 워커 전용 요청 헬퍼 (서브셸에서 호출) ─────────────────────
# do_request와 동일하나 전역 카운터 대신 wpass/wfail로 텍스트만 출력하고,
# 응답 본문은 표준 stdout이 아닌 W_BODY/W_STATUS 변수에 담는다(워커 로컬).
wdo_request() {
    local desc="$1"
    local expected_status="$2"
    shift 2

    local tmp
    tmp=$(mktemp)

    W_STATUS=$(curl -s -o "$tmp" -w "%{http_code}" \
        -H "User-Agent: novelpia-api-test/0.1 (unofficial)" \
        "$@") || { wfail "$desc (curl 오류)"; rm -f "$tmp"; return 1; }

    W_BODY=$(cat "$tmp")
    rm -f "$tmp"

    if [[ "$W_STATUS" == "$expected_status" ]]; then
        wpass "$desc [HTTP $W_STATUS]"
        if [[ "$VERBOSE" == "1" ]]; then
            echo "       응답: ${W_BODY:0:200}"
        fi
    else
        wfail "$desc [HTTP $W_STATUS, 기대: $expected_status]"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 워커용 GET 200 단축 헬퍼
wget_page() {
    local path="$1"
    local label="$2"
    shift 2
    wdo_request "GET ${path} (${label})" "200" "$@" "${BASE_URL}${path}"
}

# set -o pipefail 환경에서 echo "$BODY" | grep -q 는 grep이 매칭 후 파이프를 닫을 때
# echo가 SIGPIPE로 실패하며 파이프 전체가 non-zero로 처리되어 오판정이 발생한다.
# contains()는 파이프 없이 bash 패턴 매칭만 사용하므로 이 문제를 회피한다.
contains() { [[ "$1" == *"$2"* ]]; }

# 유료 콘텐츠 접근 차단(구매 유도/오류 모달) 응답 여부
body_is_paywall() { contains "$BODY" "alert_modal" ||
                    contains "$BODY" "coin" ||
                    contains "$BODY" "plus" ||
                    contains "$BODY" "구매" ||
                    contains "$BODY" "열람권"; }
# 뷰어 컨테이너 HTML이 정상 반환됐는지 여부
body_is_viewer()  { contains "$BODY" "novel-viewer" ||
                    contains "$BODY" "viewer_wrap" ||
                    contains "$BODY" "viewer-content"; }
# 비로그인/세션 만료 안내 메시지 여부
body_needs_login() { contains "$BODY" "로그인이 필요합니다"; }

# 워커용 변형 — 전역 $BODY 대신 $W_BODY를 검사
body_is_paywall_w() { contains "$W_BODY" "alert_modal" ||
                      contains "$W_BODY" "coin" ||
                      contains "$W_BODY" "plus" ||
                      contains "$W_BODY" "구매" ||
                      contains "$W_BODY" "열람권"; }

# 파이프 응답의 첫 필드 검증 (on/off/login)
check_pipe_response() {
    local desc="$1"
    local body="$2"
    local first_field=$(echo "$body" | cut -d'|' -f1)
    case "$first_field" in
        on|off)   info "토글 결과: $first_field" ;;
        login)    info "비로그인 응답: login|0||" ;;
        *)        fail "$desc: 예상치 못한 파이프 응답 첫 필드 '$first_field'"; return ;;
    esac
}

# ═══════════════════════════════════════════════════════════════
# 1. 공개 HTML 페이지 (인증 불필요)
# ═══════════════════════════════════════════════════════════════
echo ""
echo "━━━ 1. 공개 HTML 페이지 (병렬, MAX_JOBS=${MAX_JOBS}) ━━━━━━━━━━━━━━━━"

par_begin

par_run wget_page "/" "홈 페이지"

# 섹션별 메인 및 필터링 페이지
for section_meta in "freestory:자유 연재:/all" "plus:PLUS:"; do
    sec="${section_meta%%:*}"; rest="${section_meta#*:}"
    sec_label="${rest%%:*}"; suffix="${rest##*:}"
    par_run wget_page "/${sec}" "${sec_label} 메인"
    for sort_label in "date:기본" "view:조회순" "vote:추천순"; do
        sort="${sort_label%%:*}"; label="${sort_label##*:}"
        par_run wget_page "/${sec}/all/${sort}/1${suffix}" "${sec_label} 필터링 - ${label}"
    done
done

par_run wget_page "/top100" "TOP 100 메인"

# TOP 100 옵션 조합: {기간}/{정렬}/{연령}/{연재유형}
# 기간 변형
for period_label in "today:일간" "weekly:주간" "month:월간"; do
    period="${period_label%%:*}"; label="${period_label##*:}"
    par_run wget_page "/top100/all/${period}/view/all/all" "TOP 100 - ${label}"
done

# 정렬 변형 (기간 고정: today)
for sort_label in "vote:추천순" "like:선호순" "pick:인생픽순"; do
    sort="${sort_label%%:*}"; label="${sort_label##*:}"
    par_run wget_page "/top100/all/today/${sort}/all/all" "TOP 100 - ${label}"
done

# 연령 변형 (기간 고정: today, 정렬 고정: view)
for age_label in "teen:청소년 이용가" "adult:성인"; do
    age="${age_label%%:*}"; label="${age_label##*:}"
    par_run wget_page "/top100/all/today/view/${age}/all" "TOP 100 - ${label}"
done

# 연재 유형 변형 (기간 고정: today, 정렬 고정: view, 연령 고정: all)
for pub_label in "free:자유 연재" "plus:PLUS 연재"; do
    pub="${pub_label%%:*}"; label="${pub_label##*:}"
    par_run wget_page "/top100/all/today/view/all/${pub}" "TOP 100 - ${label}"
done

par_run wdo_request "GET /search?keyword=판타지 (소설 검색)" "200" \
    "${BASE_URL}/search?keyword=%ED%8C%90%ED%83%80%EC%A7%80"
par_run wget_page "/novel/${NOVEL_NO}" "소설 상세"
par_run wget_page "/notice" "공지사항 목록"
par_run wget_page "/page/login" "로그인 페이지"
par_run wget_page "/page/terms_of_use" "이용약관"

for arena_label in "all:전체" "32:작품 리뷰" "33:작품 홍보"; do
    arena="${arena_label%%:*}"; label="${arena_label##*:}"
    par_run wget_page "/arena/${arena}" "아레나 ${label}"
done

par_end

# ═══════════════════════════════════════════════════════════════
# 2. proc 엔드포인트 — 인증 불필요
# ═══════════════════════════════════════════════════════════════
echo ""
echo "━━━ 2. proc 엔드포인트 (공개, 병렬, MAX_JOBS=${MAX_JOBS}) ━━━━━━━━━━━"

# 모두 인증 불필요·멱등(비로그인 토글은 login 응답만 반환)이라 동시 실행 안전.
# 각 워커는 wdo_request로 W_BODY/W_STATUS를 받아 자체 검증 후 wpass/wfail/info 출력.

# 2-1. 회차 조회수 (신버전 cmd) — novel_no 필수
w_episode_count_view() {
    wdo_request "POST /proc/novel — get_episode_count_view (신버전)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "cmd=get_episode_count_view&novel_no=${NOVEL_NO}&episode_arr[0]=${EP_CODE}" \
        "${BASE_URL}/proc/novel"
    if echo "$W_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 'list' in d" 2>/dev/null; then
        info "JSON 파싱 성공, list 필드 존재"
    else
        wfail "POST /proc/novel — JSON 파싱 실패 또는 list 필드 없음"
    fi
}

# 2-2. 회차 조회수 (구버전 cmd) — novel_no 필수
w_episode_cnt_view() {
    wdo_request "POST /proc/novel — get_episode_cnt_view (구버전)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "cmd=get_episode_cnt_view&novel_no=${NOVEL_NO}&episode_arr[0]=${EP_CODE}" \
        "${BASE_URL}/proc/novel"
}

# 2-3. 회차 목록 — NOVEL_NO(기본: 23)는 회차 보유 → ep_style 없으면 실패
#             비교군 EPISODE_LIST_NOVEL_NO(기본: 31631)는 회차 미보유 → ep_style 없으면 정상(pass)
# 인수: <novel_no> <has_episodes: 1=회차 있음, 0=없음>
w_episode_list() {
    local novel_no="$1"
    local has_episodes="$2"
    local label=$([ "$has_episodes" = "1" ] && echo "회차 있는 소설" || echo "회차 없는 소설")

    wdo_request "POST /proc/episode_list — novel_no=${novel_no} (${label}, page 0)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "novel_no=${novel_no}&sort=DOWN&page=0" \
        "${BASE_URL}/proc/episode_list"

    if contains "$W_BODY" "ep_style"; then
        local note=$([ "$has_episodes" = "1" ] && echo "정상" || echo "예상보다 회차가 있음")
        info "HTML 조각 확인: ep_style 클래스 존재 ($note)"
    else
        local msg="POST /proc/episode_list — novel_no=${novel_no}: ep_style 미발견"
        local note=$([ "$has_episodes" = "1" ] && echo "예상치 못한 응답" || echo "정상")
        if [ "$has_episodes" = "1" ]; then
            wfail "$msg (${label}에서 $note)"
        else
            wpass "$msg (${label}, $note)"
        fi
    fi
}

# 2-4. 회차 뷰어 데이터 (무료 회차) — Referer 헤더 필수
w_viewer_data_free() {
    wdo_request "POST /proc/viewer_data/${EP_CODE} (무료 회차)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "Referer: ${BASE_URL}/viewer/${EP_CODE}" \
        -d "size=14" \
        "${BASE_URL}/proc/viewer_data/${EP_CODE}"
    if echo "$W_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 's' in d and 'c' in d" 2>/dev/null; then
        local line_count
        line_count=$(echo "$W_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['s']))" 2>/dev/null || echo "?")
        info "JSON 파싱 성공 — s, c 필드 존재 (${line_count}줄)"
    elif [[ -z "$W_BODY" ]]; then
        wfail "POST /proc/viewer_data — 빈 응답 (Referer 헤더 누락 또는 EP_CODE 오류)"
    elif body_is_paywall_w || contains "$W_BODY" "modal"; then
        info "오류 모달 HTML 반환 (유료 회차이거나 잘못된 EP_CODE일 수 있음)"
    else
        wfail "POST /proc/viewer_data — 알 수 없는 응답 형식"
    fi
}

# 2-5. 뷰어 데이터 — 유료 회차 (비로그인: 빈 응답 또는 오류)
w_viewer_data_paid() {
    wdo_request "POST /proc/viewer_data/${PAID_EP_CODE} (유료 회차, 비로그인)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "Referer: ${BASE_URL}/viewer/${PAID_EP_CODE}" \
        -d "size=14" \
        "${BASE_URL}/proc/viewer_data/${PAID_EP_CODE}"
    if echo "$W_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 's' in d" 2>/dev/null; then
        info "경고: 유료 회차인데 본문 데이터가 반환됨 (실제로 무료인 회차일 수 있음)"
    elif [[ -z "$W_BODY" ]]; then
        info "유료 회차 비로그인 — 빈 응답 (정상)"
    elif body_is_paywall_w; then
        info "유료 회차 비로그인 — 오류/구매 유도 모달 HTML (정상)"
    else
        info "응답: ${W_BODY:0:100}"
    fi
}

# 2-6. 뷰어 페이지 — 유료 회차 (비로그인)
# 성공 여부와 무관하게 HTML을 반환하나, 비로그인 실패 시 '로그인이 필요합니다' 오류 메시지를 포함해야 함
w_viewer_page_paid() {
    wget_page "/viewer/${PAID_EP_CODE}" "유료 회차 뷰어, 비로그인"
    if contains "$W_BODY" "로그인이 필요합니다"; then
        info "유료 회차 뷰어 비로그인 — '로그인이 필요합니다' 오류 메시지 확인 (정상)"
    else
        wfail "GET /viewer/${PAID_EP_CODE} — 비로그인 오류 메시지 '로그인이 필요합니다' 미발견"
        info "응답: ${W_BODY:0:200}"
    fi
}

# 2-7. 알람 토글 — 비로그인 시 login|0||
w_novel_alarm_anon() {
    wdo_request "POST /proc/novel_alarm — 비로그인 응답 확인" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "novel_no=${NOVEL_NO}" \
        "${BASE_URL}/proc/novel_alarm"
    if [[ "$W_BODY" == "login|0||" ]]; then
        info "비로그인 응답 정확: login|0||"
    else
        info "응답: ${W_BODY:0:100} (로그인 상태이거나 응답 형식 변경)"
    fi
}

# 2-8. 선호 토글 — 비로그인 시 login|0||
w_novel_like_anon() {
    wdo_request "POST /proc/novel_like — 비로그인 응답 확인" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "novel_no=${NOVEL_NO}&csrf=invalid" \
        "${BASE_URL}/proc/novel_like"
    local first_field
    first_field=$(echo "$W_BODY" | cut -d'|' -f1)
    if [[ "$first_field" == "login" ]]; then
        info "비로그인 응답 정확: login|0||"
    else
        wfail "POST /proc/novel_like — 비로그인 응답 오류: '${W_BODY:0:100}'"
    fi
}

# 2-9. 소설 큐레이션 — 작가의 다른 작품 목록
w_novel_curation_writer_other() {
    wdo_request "GET /proc/novel_curation — writer_other_novel (page 1)" "200" \
        "${BASE_URL}/proc/novel_curation?mem_no=17128&novel_no=${NOVEL_NO}&page=1&cmd=writer_other_novel"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'writer_other_novel' in d, 'writer_other_novel 필드 없음'
won = d['writer_other_novel']
assert 'is_next_page' in won, 'is_next_page 필드 없음'
assert 'list' in won, 'list 필드 없음'
" 2>/dev/null; then
        local count
        count=$(echo "$W_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['writer_other_novel']['list']))" 2>/dev/null || echo "?")
        info "JSON 파싱 성공 — writer_other_novel.list ${count}편"
    else
        wfail "GET /proc/novel_curation — JSON 파싱 실패 또는 필드 누락"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-11. 이모티콘 오픈스토어 — 작가 이모티콘 조회
w_emoticon_openstore_writer() {
    wdo_request "GET /proc/emoticon_openstore — getWriterEmoticon" "200" \
        "${BASE_URL}/proc/emoticon_openstore?mode=getWriterEmoticon&novel_no=${NOVEL_NO}"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in (200, '200'), 'status != 200'
assert 'emoticon_group' in d, 'emoticon_group 필드 없음'
assert 'button_show' in d, 'button_show 필드 없음'
" 2>/dev/null; then
        local group
        group=$(echo "$W_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('emoticon_group',''))" 2>/dev/null || echo "?")
        info "JSON 파싱 성공 — emoticon_group=${group}"
    else
        wfail "GET /proc/emoticon_openstore — JSON 파싱 실패 또는 필드 누락"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-10. 소설 큐레이션 — 회차 뷰어 장르 기반 추천 목록
w_novel_curation_epi_list() {
    wdo_request "GET /proc/novel_curation — epi_list_curation (main_genre=3)" "200" \
        "${BASE_URL}/proc/novel_curation?cmd=epi_list_curation&main_genre=3&novel_no=${NOVEL_NO}"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'curation_list' in d, 'curation_list 필드 없음'
assert isinstance(d['curation_list'], list), 'curation_list가 배열이 아님'
for item in d['curation_list']:
    assert 'novel_no' in item, 'novel_no 필드 없음'
    assert 'mem_admin' in item, 'mem_admin 필드 없음'
" 2>/dev/null; then
        local count
        count=$(echo "$W_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['curation_list']))" 2>/dev/null || echo "?")
        info "JSON 파싱 성공 — curation_list ${count}편"
    else
        wfail "GET /proc/novel_curation — JSON 파싱 실패 또는 필드 누락"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-12. /proc/user — get_member2 (공개)
w_user_get_member2() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member2 (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member2&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'result' in d, 'result 필드 없음'
" 2>/dev/null; then
        info "JSON 파싱 성공 — result 필드 존재"
    else
        wfail "POST /proc/user — get_member2: JSON 파싱 실패 또는 result 필드 없음"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-13. /proc/user — get_member_view (공개)
w_user_get_member_view() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_view (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_view&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'result' in d, 'result 필드 없음'
" 2>/dev/null; then
        info "JSON 파싱 성공 — result 필드 존재"
    else
        wfail "POST /proc/user — get_member_view: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-14. /proc/user — get_member_writer_novel (공개)
w_user_get_member_writer_novel() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_writer_novel (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_writer_novel&mem_no=${mem_no}&paging%5BrowCount%5D=5&paging%5BcurPage%5D=1&paging%5Border%5D=date&paging%5Bsort%5D%5Bdate%5D=1" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'result' in d, 'result 필드 없음'
" 2>/dev/null; then
        info "JSON 파싱 성공 — result 필드 존재"
    else
        wfail "POST /proc/user — get_member_writer_novel: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-15. /proc/user — get_member_badge (공개)
w_user_get_member_badge() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_badge (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_badge&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_member_badge: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-16. /proc/user — get_member_emoticon (공개)
w_user_get_member_emoticon() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_emoticon (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_emoticon&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_member_emoticon: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-17. /proc/user — get_member_stamp (공개)
w_user_get_member_stamp() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_stamp (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_stamp&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_member_stamp: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-18. /proc/user — get_member_keep_novel (공개)
w_user_get_member_keep_novel() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_keep_novel (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_keep_novel&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_member_keep_novel: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-19. /proc/user — get_member_donation (공개)
w_user_get_member_donation() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_member_donation (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_member_donation&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_member_donation: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-20. /proc/user — get_episode_cnt (공개)
w_user_get_episode_cnt() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    wdo_request "POST /proc/user — get_episode_cnt (mem_no=${mem_no})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_episode_cnt&mem_no=${mem_no}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_episode_cnt: JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-21. /proc/user — get_stat_hall_of_fame (공개, cate=emoticon/donation/episode)
w_user_get_stat_hall_of_fame() {
    local mem_no="${PROFILE_MEM_NO:-4169856}"
    local cate="$1"
    wdo_request "POST /proc/user — get_stat_hall_of_fame (cate=${cate})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=get_stat_hall_of_fame&mem_no=${mem_no}&cate=${cate}" \
        "${BASE_URL}/proc/user"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        wfail "POST /proc/user — get_stat_hall_of_fame (${cate}): JSON 파싱 실패"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-22. /proc/alarm — getAlarmCnt (비로그인: 0 또는 오류)
w_alarm_get_cnt_anon() {
    wdo_request "POST /proc/alarm — getAlarmCnt (비로그인)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -d "mode=getAlarmCnt" \
        "${BASE_URL}/proc/alarm"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        info "비로그인 응답: ${W_BODY:0:100}"
    fi
}

# 2-23. GET /proc/novel?cmd=get_novel_review_list (공개)
w_novel_get_review_list() {
    wdo_request "GET /proc/novel — get_novel_review_list (novel_no=${NOVEL_NO})" "200" \
        "${BASE_URL}/proc/novel?cmd=get_novel_review_list&target_novel_no=${NOVEL_NO}"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert isinstance(d.get('data'), list), 'data 필드가 배열이 아님'
" 2>/dev/null; then
        info "JSON 파싱 성공, data 배열 존재"
    else
        wfail "GET /proc/novel — JSON 파싱 실패 또는 data 필드 없음"
    fi
}

# 2-24. GET /proc/member_plus?cmd=event_list (공개)
w_member_plus_event_list() {
    wdo_request "GET /proc/member_plus — event_list" "200" \
        "${BASE_URL}/proc/member_plus?cmd=event_list"
    if echo "$W_BODY" | python3 -c "import sys, json; json.load(sys.stdin)" 2>/dev/null; then
        info "JSON 파싱 성공"
    else
        info "응답: ${W_BODY:0:100} (JSON이 아닐 수 있음)"
    fi
}

# 2-25. GET /proc/emoticon_proc?mode=get_user_emoticon_group (인증 필요, 비로그인 시 빈 배열)
w_emoticon_get_user_group_anon() {
    wdo_request "GET /proc/emoticon_proc — get_user_emoticon_group (비로그인)" "200" \
        "${BASE_URL}/proc/emoticon_proc?mode=get_user_emoticon_group"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in (200, '200'), 'status != 200'
assert isinstance(d.get('data'), list), 'data 필드가 배열이 아님'
" 2>/dev/null; then
        local cnt
        cnt=$(echo "$W_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['data']))" 2>/dev/null || echo "?")
        info "JSON 파싱 성공 — data 배열 ${cnt}개 (비로그인이면 0)"
    else
        wfail "GET /proc/emoticon_proc — JSON 파싱 실패 또는 data 필드 없음"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

# 2-26. GET /proc/emoticon_proc?mode=get_user_emoticon (비로그인: 본문 status=401)
# 그룹 목록(get_user_emoticon_group)과 달리 비로그인 시 빈 배열이 아닌 401을 반환한다.
w_emoticon_get_user_emoticon_anon() {
    wdo_request "GET /proc/emoticon_proc — get_user_emoticon (비로그인, status=401 기대)" "200" \
        "${BASE_URL}/proc/emoticon_proc?mode=get_user_emoticon&emoticon_group_no=14"
    if echo "$W_BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in (401, '401'), 'status != 401'
" 2>/dev/null; then
        info "비로그인 응답 정확: status=401 (로그인이 필요합니다.)"
    else
        wfail "GET /proc/emoticon_proc — get_user_emoticon: 비로그인 status=401 미확인"
        echo "       응답: ${W_BODY:0:200}"
    fi
}

par_begin
par_run w_episode_count_view
par_run w_episode_cnt_view
par_run w_episode_list "$NOVEL_NO" "1"
par_run w_episode_list "$EPISODE_LIST_NOVEL_NO" "0"
par_run w_viewer_data_free
par_run w_viewer_data_paid
par_run w_viewer_page_paid
par_run w_novel_alarm_anon
par_run w_novel_like_anon
par_run w_novel_curation_writer_other
par_run w_novel_curation_epi_list
par_run w_emoticon_openstore_writer
par_run w_user_get_member2
par_run w_user_get_member_view
par_run w_user_get_member_writer_novel
par_run w_user_get_member_badge
par_run w_user_get_member_emoticon
par_run w_user_get_member_stamp
par_run w_user_get_member_keep_novel
par_run w_user_get_member_donation
par_run w_user_get_episode_cnt
par_run w_user_get_stat_hall_of_fame "emoticon"
par_run w_user_get_stat_hall_of_fame "donation"
par_run w_user_get_stat_hall_of_fame "episode"
par_run w_alarm_get_cnt_anon
par_run w_novel_get_review_list
par_run w_member_plus_event_list
par_run w_emoticon_get_user_group_anon
par_run w_emoticon_get_user_emoticon_anon
par_end

# ═══════════════════════════════════════════════════════════════
# 3. 인증 필요 엔드포인트 (LOGINKEY 쿠키)
# ═══════════════════════════════════════════════════════════════
echo ""
echo "━━━ 3. 인증 필요 엔드포인트 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [[ -z "${LOGINKEY:-}" ]]; then
    skip "LOGINKEY 미설정 — 섹션 3 전체 생략"
    skip "GET /mybook"
    skip "GET /alarm"
    skip "POST /proc/alarm — getAlarmCnt (로그인)"
    skip "POST /proc/user — get_member_favorite_novel"
    skip "POST /proc/user — get_user_block_chk (mem_no=${PROFILE_MEM_NO})"
    skip "POST /proc/user — get_user_block_chk (mem_no=${UNBLOCKED_MEM_NO}, 비차단 기대)"
    skip "POST /proc/viewer_board_comment — get_user_block (mem_no=${PROFILE_MEM_NO})"
    skip "GET /proc/emoticon_proc — get_user_emoticon_group (로그인)"
    skip "POST /proc/novel_alarm — 로그인 상태 토글"
    skip "POST /proc/novel_like — 로그인 상태 토글 (CSRF 필요)"
    skip "POST /proc/board_option — vote_novel (CSRF 필요)"
else
    COOKIE_HEADER="Cookie: LOGINKEY=${LOGINKEY}"

    # LOGINKEY 유효성 선검증 — 무효한 키(예: fake_key)는 서버가 비로그인으로 처리하여
    # 인증 엔드포인트도 HTTP 200을 반환하므로, 상태 코드만으로는 통과한다.
    #
    # 주의: /mybook·/alarm 등 인증 페이지의 HTML에는 로그인 여부와 무관하게
    # 주석 처리된 안내 문구(//alert("로그인 후 이용 가능합니다.");)가 항상 포함되어 있어
    # 해당 문구 grep으로는 유효성을 판별할 수 없다(유효 키도 무효로 오판됨).
    # 대신 /alarm 페이지는 비로그인 시에만 '/?login_req=1' 리다이렉트 스크립트를 내려주므로
    # 이를 신뢰 가능한 서버측 비로그인 신호로 사용한다.
    AUTH_CHECK=$(curl -s -H "User-Agent: novelpia-api-test/0.1 (unofficial)" \
        -H "$COOKIE_HEADER" "${BASE_URL}/alarm")
    if contains "$AUTH_CHECK" "login_req=1"; then
        LOGINKEY_VALID=0
        fail "LOGINKEY 무효 — 서버가 비로그인으로 처리함 (섹션 3 인증 테스트 전체 생략)"
        skip "GET /mybook"
        skip "GET /alarm"
        skip "POST /proc/alarm — getAlarmCnt (로그인)"
        skip "POST /proc/user — get_member_favorite_novel"
        skip "POST /proc/user — get_user_block_chk (mem_no=${PROFILE_MEM_NO})"
        skip "POST /proc/user — get_user_block_chk (mem_no=${UNBLOCKED_MEM_NO}, 비차단 기대)"
        skip "POST /proc/viewer_board_comment — get_user_block (mem_no=${PROFILE_MEM_NO})"
        skip "GET /proc/emoticon_proc — get_user_emoticon_group (로그인)"
        skip "POST /proc/viewer_data — 유료 회차 (로그인)"
        skip "POST /proc/novel_alarm — 로그인 상태 토글"
        skip "POST /proc/novel_like — 로그인 상태 토글 (CSRF 필요)"
        skip "POST /proc/board_option — vote_novel (CSRF 필요)"
        skip "POST /proc/member_block — 차단 토글"
    else
    LOGINKEY_VALID=1

    get_page "/mybook" "내 서재" -H "$COOKIE_HEADER"

    get_page "/alarm" "알람 목록" -H "$COOKIE_HEADER"

    # 알람 카운트 (인증 상태)
    do_request "POST /proc/alarm — getAlarmCnt (로그인)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "mode=getAlarmCnt" \
        "${BASE_URL}/proc/alarm"
    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        CNT=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('cnt','?'))" 2>/dev/null || echo "?")
        info "알람 카운트: ${CNT}"
    else
        fail "POST /proc/alarm — getAlarmCnt: JSON 파싱 실패"
    fi

    # 차단 여부 확인 (인증 필요)
    PROFILE_MEM_NO="${PROFILE_MEM_NO:-4169856}"
    do_request "POST /proc/user — get_user_block_chk (mem_no=${PROFILE_MEM_NO})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "mode=get_user_block_chk&mem_no=${PROFILE_MEM_NO}" \
        "${BASE_URL}/proc/user"
    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        IS_BLOCKED=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print('차단됨' if d.get('result') else '차단 안 됨')" 2>/dev/null || echo "?")
        info "차단 상태: ${IS_BLOCKED}"
    else
        fail "POST /proc/user — get_user_block_chk: JSON 파싱 실패"
    fi

    # 차단 여부 확인 — 비차단 대상 (UNBLOCKED_MEM_NO)
    do_request "POST /proc/user — get_user_block_chk (mem_no=${UNBLOCKED_MEM_NO}, 비차단 기대)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "mode=get_user_block_chk&mem_no=${UNBLOCKED_MEM_NO}" \
        "${BASE_URL}/proc/user"
    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
" 2>/dev/null; then
        IS_BLOCKED=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print('차단됨' if d.get('result') else '차단 안 됨')" 2>/dev/null || echo "?")
        info "차단 상태 (mem_no=${UNBLOCKED_MEM_NO}): ${IS_BLOCKED}"
        if [ "$IS_BLOCKED" = "차단됨" ]; then
            warn "POST /proc/user — get_user_block_chk: mem_no=${UNBLOCKED_MEM_NO}이 차단됨 (예상: 비차단)"
        fi
    else
        fail "POST /proc/user — get_user_block_chk (mem_no=${UNBLOCKED_MEM_NO}): JSON 파싱 실패"
    fi

    # 뷰어 게시판 — 전체 차단 목록 (get_user_block)
    do_request "POST /proc/viewer_board_comment — get_user_block (mem_no=${PROFILE_MEM_NO})" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "mode=get_user_block&mem_no=${PROFILE_MEM_NO}" \
        "${BASE_URL}/proc/viewer_board_comment"
    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'result' in d, 'result 필드 없음'
assert 'user_block' in d['result'], 'result.user_block 필드 없음'
assert isinstance(d['result']['user_block'], list), 'user_block이 배열이 아님'
" 2>/dev/null; then
        BLOCK_CNT=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['result']['user_block']))" 2>/dev/null || echo "?")
        info "차단 목록 ${BLOCK_CNT}명"
    else
        fail "POST /proc/viewer_board_comment — get_user_block: JSON 파싱 실패 또는 구조 오류"
        info "응답: ${BODY:0:200}"
    fi

    # 사용자 이모티콘 그룹 목록 (인증 상태)
    do_request "GET /proc/emoticon_proc — get_user_emoticon_group (로그인)" "200" \
        -H "$COOKIE_HEADER" \
        "${BASE_URL}/proc/emoticon_proc?mode=get_user_emoticon_group"
    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert isinstance(d.get('data'), list), 'data 필드가 배열이 아님'
" 2>/dev/null; then
        EMO_CNT=$(echo "$BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['data']))" 2>/dev/null || echo "?")
        info "보유 이모티콘 그룹 ${EMO_CNT}개"
    else
        fail "GET /proc/emoticon_proc — get_user_emoticon_group: JSON 파싱 실패 또는 구조 오류"
        info "응답: ${BODY:0:200}"
    fi

    # 선호작 목록
    do_request "POST /proc/user — get_member_favorite_novel" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "mode=get_member_favorite_novel" \
        "${BASE_URL}/proc/user"

    if echo "$BODY" | python3 -c "
import sys, json
d = json.load(sys.stdin)
assert d.get('status') in ('200', 200), 'status != 200'
assert 'result' in d, 'result 필드 없음'
assert 'novel' in d['result'], 'novel 필드 없음'
" 2>/dev/null; then
        NOVEL_COUNT=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d['result']['novel']))" 2>/dev/null || echo "?")
        info "선호작 ${NOVEL_COUNT}편 확인"
    else
        fail "POST /proc/user — 응답 구조 검증 실패"
    fi

    # 유료 회차 뷰어 데이터 (인증 상태)
    do_request "POST /proc/viewer_data/${PAID_EP_CODE} (유료 회차, 로그인)" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "Referer: ${BASE_URL}/viewer/${PAID_EP_CODE}" \
        -H "$COOKIE_HEADER" \
        -d "size=14" \
        "${BASE_URL}/proc/viewer_data/${PAID_EP_CODE}"

    if echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 's' in d and 'c' in d" 2>/dev/null; then
        LINE_COUNT=$(echo "$BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['s']))" 2>/dev/null || echo "?")
        info "유료 회차 — JSON 파싱 성공, ${LINE_COUNT}줄 (구매/열람권 보유 확인)"
    elif [[ -z "$BODY" ]]; then
        info "유료 회차 로그인 — 빈 응답 (열람권 없거나 비구매)"
    elif body_is_paywall; then
        info "유료 회차 로그인 — 구매 유도 모달 (열람권 없음)"
    else
        info "응답: ${BODY:0:100}"
    fi

    # 알람 토글 (인증 상태)
    do_request "POST /proc/novel_alarm — 로그인 상태 토글" "200" \
        -X POST \
        -H "Content-Type: application/x-www-form-urlencoded" \
        -H "$COOKIE_HEADER" \
        -d "novel_no=${NOVEL_NO}" \
        "${BASE_URL}/proc/novel_alarm"
    check_pipe_response "novel_alarm 토글" "$BODY"

    # 선호 토글 (인증 + CSRF)
    if [[ -z "${CSRF_TOKEN:-}" ]]; then
        skip "POST /proc/novel_like — CSRF_TOKEN 미설정"
        skip "POST /proc/board_option — CSRF_TOKEN 미설정"
    else
        do_request "POST /proc/novel_like — 선호 토글" "200" \
            -X POST \
            -H "Content-Type: application/x-www-form-urlencoded" \
            -H "Referer: ${BASE_URL}/novel/${NOVEL_NO}" \
            -H "$COOKIE_HEADER" \
            -d "novel_no=${NOVEL_NO}&csrf=${CSRF_TOKEN}" \
            "${BASE_URL}/proc/novel_like"
        if [[ -z "$BODY" ]]; then
            fail "novel_like 선호 토글: 빈 응답 (CSRF 검증 실패로 토글 미작동)"
        else
            check_pipe_response "novel_like 선호 토글" "$BODY"
        fi

        do_request "POST /proc/board_option — vote_novel" "200" \
            -X POST \
            -H "Content-Type: application/x-www-form-urlencoded" \
            -H "$COOKIE_HEADER" \
            -d "option=vote_novel&value=${EP_CODE}&csrf=${CSRF_TOKEN}" \
            "${BASE_URL}/proc/board_option"
        check_pipe_response "board_option 추천 토글" "$BODY"
    fi

    # 회원 차단 (인증 + CSRF + MEMBER_NO)
    if [[ -z "${CSRF_TOKEN:-}" ]] || [[ -z "${MEMBER_NO:-}" ]]; then
        skip "POST /proc/member_block — CSRF_TOKEN 또는 MEMBER_NO 미설정"
    else
        do_request "POST /proc/member_block — 차단 토글" "200" \
            -X POST \
            -H "Content-Type: application/x-www-form-urlencoded" \
            -H "$COOKIE_HEADER" \
            -d "member_no=${MEMBER_NO}&csrf=${CSRF_TOKEN}" \
            "${BASE_URL}/proc/member_block"
        check_pipe_response "member_block 차단 토글" "$BODY"
    fi
    fi  # LOGINKEY 유효성 선검증 블록
fi

# ═══════════════════════════════════════════════════════════════
# 4. 뷰어 접근 (공개 URL)
# ═══════════════════════════════════════════════════════════════
echo ""
echo "━━━ 4. 뷰어 페이지 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

get_page "/viewer/${EP_CODE}" "무료 뷰어"

if [[ -z "${LOGINKEY:-}" ]]; then
    skip "GET /viewer/${PAID_EP_CODE} (유료 뷰어, 로그인) — LOGINKEY 미설정"
elif [[ "${LOGINKEY_VALID:-0}" != "1" ]]; then
    skip "GET /viewer/${PAID_EP_CODE} (유료 뷰어, 로그인) — LOGINKEY 무효"
else
    COOKIE_HEADER="Cookie: LOGINKEY=${LOGINKEY}"
    get_page "/viewer/${PAID_EP_CODE}" "유료 뷰어, 로그인" -H "$COOKIE_HEADER"
    # 로그인이 필요합니다 메시지는 비로그인 외에도 성인 인증 미완료 등의 경우에도 반환될 수 있어
    # 로그인 상태에서 수신하면 세션/인증 문제로 간주하되 fail 대신 info로 처리한다.
    if body_needs_login; then
        info "유료 뷰어 — '로그인이 필요합니다' 반환 (성인 인증 미완료 또는 세션 만료일 수 있음)"
    elif body_is_viewer; then
        info "유료 뷰어 — 뷰어 컨테이너 HTML 확인 (열람권 보유)"
    elif body_is_paywall; then
        info "유료 뷰어 — 구매 유도 UI (열람권 없음)"
    else
        info "유료 뷰어 응답: ${BODY:0:100}"
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 결과 요약
# ═══════════════════════════════════════════════════════════════
echo ""
echo "━━━ 결과 ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}PASS${RESET}: ${PASS}  ${RED}FAIL${RESET}: ${FAIL}  ${YELLOW}SKIP${RESET}: ${SKIP}"
echo ""

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
