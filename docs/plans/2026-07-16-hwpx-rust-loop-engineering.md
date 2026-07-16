# hwpx-rust Loop Engineering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 깨진 Clippy/fuzz 기준선을 먼저 복구하고, 동일한 base/head를 비교하는 결정적 verifier와 역할이 분리된 최소 이슈 오케스트레이터로 Dependabot PR 하나를 안전하게 평가한다.

**Architecture:** GitHub 이슈를 승인과 범위의 원장으로 사용하되 실행 상태와 hash-chain 감사 로그는 저장소 밖 `~/.local/state/hwpx-loop/`에 둔다. Rust 기준선 수정은 이슈별 worktree에서 수행하고, verifier와 orchestrator는 표준 라이브러리만 사용하는 작은 Python 모듈로 구현한다. Hermes는 스키마와 상태 코드만 처리하며, 구현과 리뷰는 반드시 서로 다른 OpenCode 세션의 `openai/gpt-5.6-sol`이 담당한다.

**Tech Stack:** Rust 1.97.0, pinned nightly `nightly-2025-06-01`, `cargo-fuzz 0.13.1`, GitHub Actions, GitHub CLI, Python 3.12 표준 라이브러리, Docker/OCI digest 고정 실행, OpenCode `openai/gpt-5.6-sol`.

---

## Spec Reference

- 승인 설계: `docs/superpowers/specs/2026-07-16-hwpx-rust-loop-engineering-design.md`
- 저장소: `Han-taz/hwpx-rust`
- 기준 브랜치: `main`
- 설계 기준 SHA: `6d1371c704853e6b427296b98fc9da4d0c5e49c6`
- stable verifier 이미지: `docker.io/library/rust:1.97.0-bookworm@sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073`

## 역할 및 실행 제약

- Hermes는 업무 배정, 이슈 계약/worker handoff 스키마 검사, 명령 실행 조정, 상태 코드와 artifact digest 확인만 한다.
- Hermes는 소스·테스트·CI·설정·문서 구현물을 작성하지 않고 diff를 코드 품질 관점에서 읽거나 리뷰 의견을 만들지 않는다.
- 구현 워커와 리뷰 워커는 모두 OpenCode의 정확한 모델 `openai/gpt-5.6-sol`을 사용한다.
- 리뷰 워커는 구현 워커와 다른 OpenCode 세션 및 다른 `worker_id`를 사용하고 파일을 수정하거나 커밋하지 않는다.
- 전체 OpenCode 워커는 최대 2개다. verifier 프로세스는 워커 수에 포함하지 않지만 저장소당 하나만 실행한다.
- P0-1과 P0-2는 변경 경로가 겹치지 않는 독립 worktree에서 구현 워커 2개로 병렬 실행할 수 있다. P0-1은 Rust 소스만, P0-2는 `.github/workflows/ci.yml`, `packages/hwpx-python/scripts/check_workflows_test.py`, `docs/fuzzing.md`만 소유한다.
- P0-3은 P0-1/P0-2의 실제 failure fingerprint와 head SHA를 받은 뒤 시작한다. P0-4, orchestrator, 제한 파일럿은 아래 순서대로 진행한다.
- GitHub 설정 변경, 이슈의 `loop:ready` 승인, CI/toolchain 정책, lockfile/직접 의존성 변경, 최종 병합은 사람 유지관리자의 SHA가 포함된 승인이 필요하다.

## File Map

Create:

- `docs/plans/2026-07-16-hwpx-rust-loop-engineering.md` - 이 실행 계획.
- `tools/hwpx_loop/__init__.py` - verifier/orchestrator의 공개 상수와 package marker.
- `tools/hwpx_loop/profile.py` - `baseline-v1`의 고정 이미지, 도구 버전, command ID와 argv.
- `tools/hwpx_loop/verifier.py` - checkout 격리, 명령 실행, fingerprint, 차등 판정, canonical JSON.
- `tools/hwpx_loop/verifier_test.py` - 여섯 delta, 전체 verdict, invalid input, digest 결정성 테스트.
- `tools/hwpx_loop/orchestrator.py` - 이슈 계약/handoff 검사, 상태 전이, 잠금, OpenCode 명령 작성, 감사 이벤트.
- `tools/hwpx_loop/orchestrator_test.py` - 역할 분리, 경로 제한, 상태 머신, 잠금, hash chain 테스트.
- `tools/hwpx_loop/cli.py` - `verify`, `validate-issue`, `validate-handoff`, `transition`, `worker-command` CLI.
- `tools/hwpx_loop/cli_test.py` - CLI JSON/종료 코드 계약 테스트.
- `docs/loop-engineering.md` - Dependency Review 결정, 운영 명령, 사람 승인·복구 절차.

Modify:

- `crates/hwp-core/src/document/docinfo/mod.rs:275-281` - `TRACK_CHANGE_AUTHOR` match guard로 Clippy 경고 제거.
- `crates/hwp-core/src/parser/hwpx/container.rs:274-299` - 0 divisor 동작을 보존하며 checked division 사용.
- `crates/hwp-core/src/parser/hwpx/section.rs:1389-1406` - hyperlink `stringParam` 조건을 match guard로 이동.
- `.github/workflows/ci.yml:110-135` - nightly와 cargo-fuzz 버전 고정 및 fuzz 단계를 독립 command로 유지.
- `.github/workflows/dependency-review.yml:18-22` - Dependency graph 활성화가 승인된 경우에만 정책 주석과 고정 action 설정을 유지한다.
- `packages/hwpx-python/scripts/check_workflows_test.py` - fuzz pin과 Dependency Review 정책 회귀 검사.
- `docs/fuzzing.md:20-31` - CI와 동일한 pinned fuzz 설치·실행 방법.

Do not modify:

- `Cargo.lock`, `fuzz/Cargo.lock`, `Cargo.toml`, `fuzz/Cargo.toml`.
- HWP/HWPX 기능 또는 공개 API.
- Dependabot PR `#8`~`#13`의 의존성 버전(마지막 제한 파일럿에서 선택한 PR의 기존 diff도 구현 워커가 수정하지 않는다).

---

### Task 1: P0 라벨과 이슈 4개 생성 계약

**Files:**
- Repository changes: none.
- External state: GitHub labels and four issues in `Han-taz/hwpx-rust`.

- [ ] **Step 1: 사람이 GitHub 변경 범위와 현재 빈 일반 이슈 목록을 확인한다 (2분)**

Run:

```bash
gh repo view Han-taz/hwpx-rust --json nameWithOwner,defaultBranchRef
gh issue list --repo Han-taz/hwpx-rust --state open --limit 100 --json number,title,labels
```

Expected: 저장소가 `Han-taz/hwpx-rust`, 기본 브랜치가 `main`, 일반 열린 이슈가 0개다. 결과가 다르면 생성하지 말고 설계를 다시 승인받는다.

- [ ] **Step 2: 라벨 dry-run 목록을 검토하고 사람이 승인을 기록한다 (3분)**

승인 대상은 아래 15개 라벨 이름과 설명 전체다. 색상은 상태 `1D76DB`, 우선순위 `B60205`, 종류 `5319E7`로 고정한다.

```text
loop:proposed, loop:ready, loop:claimed, loop:implementing, loop:verifying,
loop:reviewing, loop:repair, loop:awaiting-human, loop:blocked, loop:done,
loop:aborted, priority:p0, kind:baseline, kind:dependency, kind:infrastructure
```

Expected: 승인 기록에 approver GitHub identity, UTC 시각, 위 라벨 목록, 기준 SHA `6d1371c704853e6b427296b98fc9da4d0c5e49c6`가 포함된다.

- [ ] **Step 3: 사람이 승인 후 라벨을 생성한다 (5분)**

Run:

```bash
for spec in \
  'loop:proposed|후보, 사람 승인 전|1D76DB' \
  'loop:ready|범위와 수용 기준 승인 완료|1D76DB' \
  'loop:claimed|worktree와 잠금 확보|1D76DB' \
  'loop:implementing|구현 워커 작업 중|1D76DB' \
  'loop:verifying|deterministic verifier 실행 중|1D76DB' \
  'loop:reviewing|분리 리뷰 워커 검토 중|1D76DB' \
  'loop:repair|검증 또는 리뷰 수정 중|1D76DB' \
  'loop:awaiting-human|사람 결정 대기|1D76DB' \
  'loop:blocked|외부 의존성 또는 인프라 차단|1D76DB' \
  'loop:done|사람이 병합 또는 완료 처리|1D76DB' \
  'loop:aborted|정책에 따라 중단|1D76DB' \
  'priority:p0|기준선과 안전 장치 최우선|B60205' \
  'kind:baseline|main 기준선 복구|5319E7' \
  'kind:dependency|의존성 또는 Dependabot 처리|5319E7' \
  'kind:infrastructure|CI verifier GitHub 기능|5319E7'; do
  IFS='|' read -r name description color <<< "$spec"
  gh label create "$name" --repo Han-taz/hwpx-rust --description "$description" --color "$color"
done
```

Expected: 15개 명령이 성공한다. 이미 존재하는 라벨이 하나라도 있으면 `--force`로 덮어쓰지 말고 사람에게 중단 보고한다.

- [ ] **Step 4: P0-1 이슈를 `loop:proposed`로 생성한다 (4분)**

Run:

```bash
gh issue create --repo Han-taz/hwpx-rust \
  --title 'P0-1: main Clippy 기준선 복구' \
  --label 'loop:proposed,priority:p0,kind:baseline' \
  --body $'## 문제와 사용자 영향\n`main`의 `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`가 실패해 신규 회귀를 구분할 수 없다.\n\n## Base 재현과 관찰\nBase SHA: `6d1371c704853e6b427296b98fc9da4d0c5e49c6`\n명령: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`\n관찰: `collapsible_match` 2건(`docinfo/mod.rs`, `section.rs`)과 `manual_checked_ops` 1건(`container.rs`).\n\n## 허용/금지 경로\n허용: `crates/hwp-core/src/document/docinfo/mod.rs`, `crates/hwp-core/src/parser/hwpx/container.rs`, `crates/hwp-core/src/parser/hwpx/section.rs`.\n금지: 그 외 모든 파일, lint allow 추가, `-D warnings` 제거, 공개 API 변경.\n\n## 수용 기준\n`baseline-v1`에서 base는 같은 fingerprint의 `pre_existing_failure`, head는 `improved`; 나머지 명령에 신규/변경 실패가 없다. 기존 관련 unit test가 통과한다.\n\n## Verifier profile\n`baseline-v1`, stable Rust 1.97.0, OCI digest `sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073`.\n\n## 선행/충돌\n선행 없음. P0-2 허용 경로와 겹치지 않아 최대 2개 구현 워커로 병렬 가능.\n\n## 사람 승인 필요\n공개 API, 직렬화 형식, Python API 영향 또는 허용 경로 확대.\n\n## 예산\nrepair 최대 2회, 구현/repair 시도당 45분.\n\n## 롤백\n이 이슈 브랜치의 커밋만 revert하고 worktree/로그를 보존한다.'
```

Expected: 새 이슈 URL 하나를 출력하며 상태 라벨은 정확히 `loop:proposed` 하나다.

- [ ] **Step 5: P0-2 이슈를 `loop:proposed`로 생성한다 (4분)**

Run:

```bash
gh issue create --repo Han-taz/hwpx-rust \
  --title 'P0-2: fuzz 도구체인과 target build 기준선 복구' \
  --label 'loop:proposed,priority:p0,kind:baseline,kind:infrastructure' \
  --body $'## 문제와 사용자 영향\n가변 nightly와 무버전 cargo-fuzz 설치 때문에 설치 실패와 fuzz workspace 실패가 혼동된다.\n\n## Base 재현과 관찰\nBase SHA: `6d1371c704853e6b427296b98fc9da4d0c5e49c6`. CI run `27080296767`에서 cargo-fuzz 0.13.1 설치가 rustix 0.36.5와 당시 nightly 비호환으로 실패했다. 명령은 `cargo install cargo-fuzz --locked`, `cargo check --manifest-path fuzz/Cargo.toml --locked`, `cargo fuzz list`, 두 target build다.\n\n## 허용/금지 경로\n허용: `.github/workflows/ci.yml`, `packages/hwpx-python/scripts/check_workflows_test.py`, `docs/fuzzing.md`. 금지: fuzz target 삭제, build 생략/실패 무시, Cargo lockfile/manifest 변경.\n\n## 수용 기준\n`nightly-2025-06-01`과 `cargo-fuzz 0.13.1`을 고정하고 workspace check, list, `parse_auto`, `parse_hwpx` build가 깨끗한 환경에서 두 번 연속 같은 성공 결과를 낸다.\n\n## Verifier profile\n`baseline-v1-fuzz`, nightly `2025-06-01`, cargo-fuzz `0.13.1`, 명령별 독립 command ID.\n\n## 선행/충돌\n선행 없음. P0-1 허용 경로와 겹치지 않아 독립 worktree에서 병렬 가능.\n\n## 사람 승인 필요\n이 nightly/cargo-fuzz pin과 CI 변경을 구현 전에 승인. lockfile/manifest 변경은 별도 재승인 없이는 금지.\n\n## 예산\nrepair 최대 2회, 구현/repair 시도당 45분.\n\n## 롤백\nworkflow/test/doc 커밋만 revert하고 이전 CI run과 새 로그를 보존한다.'
```

Expected: 새 이슈 URL 하나를 출력한다.

- [ ] **Step 6: P0-3과 P0-4 이슈를 생성한다 (5분)**

Run:

```bash
gh issue create --repo Han-taz/hwpx-rust --title 'P0-3: base/head 차등 verifier 최소 구현' --label 'loop:proposed,priority:p0,kind:infrastructure' --body $'## 문제와 사용자 영향\n서로 다른 시점의 base/head 체크로 기존 실패와 회귀를 구분할 수 없다.\n\n## Base 재현과 관찰\n`gh pr view 11 --json baseRefOid,headRefOid,statusCheckRollup`에서 저장된 과거 결과만 제공하며 동일 환경 재실행 계약이 없다.\n\n## 허용/금지 경로\n허용: `tools/hwpx_loop/__init__.py`, `profile.py`, `verifier.py`, `verifier_test.py`, `cli.py`, `cli_test.py`. 금지: 기존 CI 결과를 pass로 간주, dirty checkout, 가변 toolchain/image.\n\n## 수용 기준\n합성 fixture로 `unchanged_pass`, `improved`, `pre_existing_failure`, `changed_failure`, `new_regression`, `inconclusive`와 invalid input을 재현한다. 같은 입력은 byte-identical canonical JSON SHA-256을 만든다. 종료 코드는 0/20/21/30/40 계약을 지킨다.\n\n## Verifier profile\n`baseline-v1`; stable OCI digest `sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073`; P0-2 pin을 fuzz command에 사용.\n\n## 선행/충돌\nP0-1/P0-2의 실제 fingerprint와 head SHA 필요.\n\n## 사람 승인 필요\nprofile 명령/timeout/네트워크 정책 또는 허용 경로 변경.\n\n## 예산\nrepair 최대 2회, 시도당 45분.\n\n## 롤백\n`tools/hwpx_loop` verifier 커밋을 revert하고 run artifact를 보존한다.'

gh issue create --repo Han-taz/hwpx-rust --title 'P0-4: Dependency Review 기반 기능 정책 적용' --label 'loop:proposed,priority:p0,kind:infrastructure,kind:dependency' --body $'## 문제와 사용자 영향\n워크플로는 있으나 Dependency graph가 비활성이라 모든 PR에서 Dependency Review를 실행할 수 없다. 이를 코드 안전 통과나 코드 회귀로 오판할 수 있다.\n\n## Base 재현과 관찰\n`gh api repos/Han-taz/hwpx-rust --jq .security_and_analysis.dependency_graph.status`와 PR Dependency Review check를 확인한다. 현재 기대값은 disabled/infrastructure failure다.\n\n## 허용/금지 경로\n허용: 사람 승인에 따른 GitHub Dependency graph 설정, `.github/workflows/dependency-review.yml`, `packages/hwpx-python/scripts/check_workflows_test.py`, `docs/loop-engineering.md`. 금지: Hermes의 설정 변경, 인프라 실패를 dependency 안전 통과로 해석.\n\n## 수용 기준\n활성화 시 승인자/시각/SHA와 PR 성공을 기록한다. 비활성 유지 시 required gate에서 제외하고 verifier에 `infrastructure_unavailable`로 기록한다. 어느 선택이든 코드 delta 판정과 분리한다.\n\n## Verifier profile\nDependency Review는 `baseline-v1` 코드 delta에서 제외하며 별도 infrastructure status다.\n\n## 선행/충돌\nP0-3의 infrastructure 결과 필드가 필요하다.\n\n## 사람 승인 필요\nDependency graph, required check, workflow 변경 모두 필요.\n\n## 예산\nrepair 최대 2회, 시도당 45분.\n\n## 롤백\n설정 활성화 시 사람이 원상 복구하고 workflow/doc 커밋을 revert한다. 감사 이벤트는 삭제하지 않는다.'
```

Expected: 총 네 P0 이슈가 생성되고 모두 `loop:proposed`다. 이 단계에서는 `loop:ready`로 바꾸지 않는다.

- [ ] **Step 7: 이슈 계약을 기계적으로 확인한다 (3분)**

Run:

```bash
gh issue list --repo Han-taz/hwpx-rust --state open --limit 20 --json number,title,body,labels > /tmp/hwpx-loop-p0-issues.json
python3 - <<'PY'
import json
p = json.load(open('/tmp/hwpx-loop-p0-issues.json'))
required = ['문제와 사용자 영향', 'Base 재현과 관찰', '허용/금지 경로', '수용 기준', 'Verifier profile', '선행/충돌', '사람 승인 필요', '예산', '롤백']
items = [i for i in p if i['title'].startswith('P0-')]
assert len(items) == 4
for item in items:
    assert all(field in item['body'] for field in required), item['title']
    states = [x['name'] for x in item['labels'] if x['name'].startswith('loop:')]
    assert states == ['loop:proposed'], (item['title'], states)
print('P0 issue contracts: PASS')
PY
```

Expected: `P0 issue contracts: PASS`.

---

### Task 2: P0-1 Clippy 기준선 복구

**Files:**
- Modify: `crates/hwp-core/src/document/docinfo/mod.rs:275-281`
- Modify: `crates/hwp-core/src/parser/hwpx/container.rs:274-299`
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs:1389-1406`

- [ ] **Step 1: 사람 승인 후 독립 worktree와 구현 워커 A를 확보한다 (3분)**

Run (사람이 승인한 실제 이슈 번호를 `P01`에 넣는다):

```bash
P01=$(gh issue list --repo Han-taz/hwpx-rust --search 'P0-1 in:title' --json number --jq '.[0].number')
git worktree add "../issue-${P01}-impl" -b "loop/issue-${P01}-clippy-baseline" 6d1371c704853e6b427296b98fc9da4d0c5e49c6
```

Expected: `$P01` 숫자가 들어간 clean `../issue-${P01}-impl` worktree. 원자 잠금을 얻은 뒤에만 별도 OpenCode 세션 `worker_id=impl-clippy-01`, model `openai/gpt-5.6-sol`을 시작한다.

- [ ] **Step 2: RED 명령으로 세 diagnostic을 재현한다 (5분)**

Run:

```bash
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Expected: FAIL. `docinfo/mod.rs:276`와 `section.rs:1392`에서 `clippy::collapsible_match`, `container.rs:283`에서 `clippy::manual_checked_ops`가 보고된다.

- [ ] **Step 3: 기존 동작 테스트가 먼저 통과함을 확인한다 (4분)**

Run:

```bash
cargo test -p hwp-core parser::hwpx::container::tests::rejects_entry_with_excessive_compression_ratio --lib --locked
cargo test -p hwp-core parser::hwpx::container::tests::accepts_empty_entry_with_zero_compressed_size --lib --locked
cargo test -p hwp-core parser::hwpx::section::tests::test_parse_hyperlink --lib --locked
```

Expected: 세 명령 모두 PASS. 이 작업은 lint-only 변환이므로 새 기능 테스트 대신 기존 경계/하이퍼링크 테스트를 보존 계약으로 사용한다.

- [ ] **Step 4: 세 warning만 최소 수정한다 (5분)**

Apply exactly these transformations:

```rust
// docinfo/mod.rs
HwpTag::TRACK_CHANGE_AUTHOR if header.level == 1 => {
    let track_change_author = TrackChangeAuthor::parse(record_data)?;
    doc_info.track_change_authors.push(track_change_author);
}
```

```rust
// container.rs; uncompressed_size == 0 early return remains unchanged.
let ratio = (uncompressed_size - 1)
    .checked_div(compressed_size)
    .map_or(u64::MAX, |quotient| quotient + 1);
```

```rust
// section.rs
s if (s.ends_with(b":stringParam") || s == b"stringParam")
    && in_parameters
    && hyperlink_state.active =>
{
    for_each_xml_attribute(&section_source, e, |attr| {
        let key = attr.key.as_ref();
        if key == b"name" {
            current_param_name = parse_string_attr(
                &section_source,
                "hp:stringParam",
                "name",
                &attr,
            )?;
        }
        Ok(())
    })?;
}
```

Do not add `#[allow]`, rename symbols, or reformat unrelated matches.

- [ ] **Step 5: GREEN 명령을 실행한다 (5분)**

Run:

```bash
cargo fmt --all -- --check
cargo test -p hwp-core parser::hwpx::container::tests --lib --locked
cargo test -p hwp-core parser::hwpx::section::tests::test_parse_hyperlink --lib --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p hwpx-python --no-default-features --all-targets --locked -- -D warnings
```

Expected: 모두 PASS, warning 0개. Clippy 외 명령에서 신규 실패가 있으면 커밋하지 않고 repair 결과로 반환한다.

- [ ] **Step 6: 허용 경로와 diff를 확인하고 커밋한다 (3분)**

Run:

```bash
git diff --check
git status --short
git add crates/hwp-core/src/document/docinfo/mod.rs crates/hwp-core/src/parser/hwpx/container.rs crates/hwp-core/src/parser/hwpx/section.rs
git commit -m "fix: Clippy 기준선 경고 제거"
```

Expected: 위 세 파일만 포함한 커밋. 구현 handoff의 `result`는 `ready_for_verification`, model은 `openai/gpt-5.6-sol`이다.

---

### Task 3: P0-2 fuzz 도구체인과 빌드 복구

**Files:**
- Modify: `.github/workflows/ci.yml:110-135`
- Modify: `packages/hwpx-python/scripts/check_workflows_test.py`
- Modify: `docs/fuzzing.md:20-31`

이 Task는 Task 2와 독립 worktree에서 동시에 실행할 수 있다. 두 Task를 병렬 실행할 때 OpenCode 구현 워커는 정확히 2개이고 리뷰 워커는 아직 시작하지 않는다.

- [ ] **Step 1: 사람의 pin 승인을 확인하고 구현 워커 B를 시작한다 (3분)**

승인 기록은 `nightly-2025-06-01`, `cargo-fuzz 0.13.1`, 허용 파일 3개, 기준 SHA를 포함해야 한다. 아래 명령으로 실제 이슈 번호를 읽고 독립 worktree를 만든 뒤 OpenCode session `impl-fuzz-01`을 사용한다.

```bash
P02=$(gh issue list --repo Han-taz/hwpx-rust --search 'P0-2 in:title' --json number --jq '.[0].number')
git worktree add "../issue-${P02}-impl" -b "loop/issue-${P02}-fuzz-baseline" 6d1371c704853e6b427296b98fc9da4d0c5e49c6
```

Expected: P0-1 worktree와 경로가 겹치지 않으며 전체 active OpenCode worker가 2 이하이다.

- [ ] **Step 2: RED로 기존 workflow 계약이 pin을 보장하지 않음을 고정한다 (4분)**

Add to `PackagingWorkflowTest`:

```python
def test_fuzz_job_pins_toolchain_and_cargo_fuzz(self) -> None:
    fuzz = job_section(workflow_text(CI_WORKFLOW), "fuzz")

    self.assertIn("toolchain: nightly-2025-06-01", fuzz)
    self.assertIn("cargo install cargo-fuzz --version 0.13.1 --locked", fuzz)
    self.assertIn("cargo +nightly-2025-06-01 fuzz list", fuzz)
    self.assertIn("cargo +nightly-2025-06-01 fuzz build parse_auto", fuzz)
    self.assertIn("cargo +nightly-2025-06-01 fuzz build parse_hwpx", fuzz)
    self.assertNotIn("rust-toolchain@nightly\n", fuzz)
```

Run:

```bash
python3 packages/hwpx-python/scripts/check_workflows_test.py
```

Expected: FAIL because current workflow uses floating `rust-toolchain@nightly`, unversioned install, and unqualified `cargo fuzz`.

- [ ] **Step 3: workflow에 exact pin을 최소 적용한다 (4분)**

Replace only the fuzz setup/install/list/build commands:

```yaml
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly-2025-06-01

      - name: Install cargo-fuzz 0.13.1
        run: cargo install cargo-fuzz --version 0.13.1 --locked

      - name: Check fuzz workspace
        run: cargo +nightly-2025-06-01 check --manifest-path fuzz/Cargo.toml --locked

      - name: List fuzz targets
        run: cargo +nightly-2025-06-01 fuzz list

      - name: Build parse_auto fuzz target
        run: cargo +nightly-2025-06-01 fuzz build parse_auto

      - name: Build parse_hwpx fuzz target
        run: cargo +nightly-2025-06-01 fuzz build parse_hwpx
```

Keep checkout/cache unchanged. Separate build steps preserve distinct fingerprints.

- [ ] **Step 4: local 문서를 같은 exact 명령으로 바꾼다 (3분)**

In `docs/fuzzing.md`, replace setup/list/build examples with:

```bash
rustup toolchain install nightly-2025-06-01
cargo install cargo-fuzz --version 0.13.1 --locked
cargo +nightly-2025-06-01 check --manifest-path fuzz/Cargo.toml --locked
cargo +nightly-2025-06-01 fuzz list
cargo +nightly-2025-06-01 fuzz build parse_auto
cargo +nightly-2025-06-01 fuzz build parse_hwpx
```

- [ ] **Step 5: GREEN을 깨끗한 Linux container에서 두 번 실행한다 (5분 + 실행 대기)**

Run twice with a new container each time:

```bash
docker run --rm -v "$PWD:/work:ro" -w /tmp docker.io/library/rust:1.97.0-bookworm@sha256:8fa55b2f3ddf97471ab6a767bfa3f37e6bad0986ba823e75fea57e2a2a5c3073 bash -lc '
  cp -a /work repo && cd repo &&
  rustup toolchain install nightly-2025-06-01 &&
  cargo install cargo-fuzz --version 0.13.1 --locked &&
  cargo +nightly-2025-06-01 check --manifest-path fuzz/Cargo.toml --locked &&
  cargo +nightly-2025-06-01 fuzz list &&
  cargo +nightly-2025-06-01 fuzz build parse_auto &&
  cargo +nightly-2025-06-01 fuzz build parse_hwpx'
```

Expected: both runs exit 0; target list includes `parse_auto` and `parse_hwpx`; no lockfile changes. If the approved pair fails, mark `blocked`; do not silently select a new version.

- [ ] **Step 6: workflow test와 변경 범위를 확인하고 커밋한다 (3분)**

Run:

```bash
python3 packages/hwpx-python/scripts/check_workflows_test.py
git diff --check
git status --short
git add .github/workflows/ci.yml packages/hwpx-python/scripts/check_workflows_test.py docs/fuzzing.md
git commit -m "ci: fuzz 도구체인 버전 고정"
```

Expected: test PASS, 세 파일만 커밋, `Cargo.lock`과 `fuzz/Cargo.lock` unchanged.

---

### Task 4: P0-3 base/head 차등 verifier 구현

**Files:**
- Create: `tools/hwpx_loop/__init__.py`
- Create: `tools/hwpx_loop/profile.py`
- Create: `tools/hwpx_loop/verifier.py`
- Create: `tools/hwpx_loop/verifier_test.py`
- Create/Modify: `tools/hwpx_loop/cli.py`
- Create/Modify: `tools/hwpx_loop/cli_test.py`

- [ ] **Step 1: P0-1/P0-2 결과를 fixture 입력으로 기록한다 (3분)**

Run:

```bash
P01=$(gh issue list --repo Han-taz/hwpx-rust --search 'P0-1 in:title' --json number --jq '.[0].number')
P02=$(gh issue list --repo Han-taz/hwpx-rust --search 'P0-2 in:title' --json number --jq '.[0].number')
P01_HEAD=$(git rev-parse "loop/issue-${P01}-clippy-baseline")
P02_HEAD=$(git rev-parse "loop/issue-${P02}-fuzz-baseline")
printf '%s\n%s\n' "$P01_HEAD" "$P02_HEAD"
```

Expected: 두 immutable 40-character head SHA가 출력되고 issue handoff의 SHA와 일치한다.

- [ ] **Step 2: RED delta/verdict 테스트를 작성한다 (5분)**

Create `tools/hwpx_loop/verifier_test.py` with table-driven tests using these exact expectations:

```python
import unittest
from tools.hwpx_loop.verifier import CommandResult, classify_delta, overall_verdict


class DeltaTest(unittest.TestCase):
    def result(self, status: str, fingerprint: str | None = None) -> CommandResult:
        return CommandResult(status=status, exit_code=0 if status == "pass" else 1,
                             fingerprint=fingerprint, duration_ms=1)

    def test_six_delta_states(self) -> None:
        cases = [
            (self.result("pass"), self.result("pass"), "unchanged_pass"),
            (self.result("fail", "a"), self.result("pass"), "improved"),
            (self.result("fail", "a"), self.result("fail", "a"), "pre_existing_failure"),
            (self.result("fail", "a"), self.result("fail", "b"), "changed_failure"),
            (self.result("pass"), self.result("fail", "b"), "new_regression"),
            (self.result("timeout"), self.result("pass"), "inconclusive"),
        ]
        for base, head, expected in cases:
            with self.subTest(expected=expected):
                self.assertEqual(classify_delta(base, head), expected)

    def test_verdict_precedence_and_codes(self) -> None:
        self.assertEqual(overall_verdict(["improved"], True), ("candidate_pass", 0))
        self.assertEqual(overall_verdict(["new_regression"], True), ("repair_required", 20))
        self.assertEqual(overall_verdict(["pre_existing_failure"], False), ("objective_not_met", 21))
        self.assertEqual(overall_verdict(["inconclusive"], True), ("infrastructure_blocked", 30))
```

Run: `python3 -m unittest tools.hwpx_loop.verifier_test -v`

Expected: FAIL with `ModuleNotFoundError`.

- [ ] **Step 3: 최소 result/delta/verdict 타입을 구현한다 (5분)**

Create package marker and implement in `verifier.py`:

```python
from dataclasses import dataclass

@dataclass(frozen=True)
class CommandResult:
    status: str
    exit_code: int | None
    fingerprint: str | None
    duration_ms: int

def classify_delta(base: CommandResult, head: CommandResult) -> str:
    if base.status in {"timeout", "cancelled", "infrastructure_error"} or head.status in {"timeout", "cancelled", "infrastructure_error"}:
        return "inconclusive"
    if base.status == "pass" and head.status == "pass": return "unchanged_pass"
    if base.status == "fail" and head.status == "pass": return "improved"
    if base.status == "pass" and head.status == "fail": return "new_regression"
    return "pre_existing_failure" if base.fingerprint == head.fingerprint else "changed_failure"

def overall_verdict(deltas: list[str], objective_improved: bool) -> tuple[str, int]:
    if "inconclusive" in deltas: return "infrastructure_blocked", 30
    if {"new_regression", "changed_failure"} & set(deltas): return "repair_required", 20
    if objective_improved: return "candidate_pass", 0
    return "objective_not_met", 21
```

Run: `python3 -m unittest tools.hwpx_loop.verifier_test -v`

Expected: PASS.

- [ ] **Step 4: RED fingerprint와 canonical JSON 결정성 테스트를 추가한다 (5분)**

Add tests that call `fingerprint(argv, step, exit_code, stderr)` twice with paths `/tmp/a` and `/tmp/b`, ISO timestamps, and different elapsed values; assert equal digest. Add `canonical_bytes()` test asserting sorted compact UTF-8 JSON plus one trailing newline and equal SHA-256 on repeated calls.

Run: `python3 -m unittest tools.hwpx_loop.verifier_test -v`

Expected: FAIL because both functions are absent.

- [ ] **Step 5: normalization과 canonical serialization을 구현한다 (5분)**

Implement:

```python
import hashlib, json, re

def fingerprint(argv: list[str], step: str, exit_code: int, stderr: str) -> str:
    text = re.sub(r"/tmp/[A-Za-z0-9_./-]+", "<TMP>", stderr)
    text = re.sub(r"\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z\b", "<TIME>", text)
    text = re.sub(r"\b(?:finished|elapsed) in \d+(?:\.\d+)?s\b", "<DURATION>", text, flags=re.I)
    diagnostic = "\n".join(line.strip() for line in text.splitlines() if line.strip())
    raw = json.dumps([argv, step, exit_code, diagnostic], ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(raw.encode()).hexdigest()

def canonical_bytes(value: dict) -> bytes:
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
```

Expected GREEN: all verifier unit tests PASS.

- [ ] **Step 6: RED profile/input validation 테스트를 작성한다 (5분)**

Test exact `baseline-v1` command IDs: `fmt`, `test-workspace`, `test-python-embedding`, `clippy-workspace`, `clippy-python-embedding`, `rustdoc`, `benchmark-compile`, `fuzz-check`, `fuzz-list`, `fuzz-build-parse-auto`, `fuzz-build-parse-hwpx`. Assert full 40-character SHAs, clean checkout, exact OCI digest, no `stable`/`nightly`/`latest` token, unique IDs, positive timeout. Invalid cases must return `invalid_run` and exit 40 before command execution.

Run: `python3 -m unittest tools.hwpx_loop.verifier_test -v`

Expected: FAIL because `profile.py` and `validate_request` do not exist.

- [ ] **Step 7: profile과 request validation을 구현한다 (5분)**

In `profile.py`, define immutable dicts with the spec command argv, stable Rust `1.97.0`, nightly `nightly-2025-06-01`, cargo-fuzz `0.13.1`, the OCI digest in this plan header, per-command timeout seconds, `network="disabled-after-tool-bootstrap"`, and `cache="read-only-content-addressed"`. In `verifier.py`, reject non-SHA input, equal base/head, dirty repository, image/profile mismatch, moving refs, or missing command fields before creating checkouts.

Expected GREEN: request tests PASS and invalid inputs map to 40 without invoking `subprocess.run`.

- [ ] **Step 8: RED isolated runner 테스트를 작성한다 (5분)**

Mock `subprocess.run` and `tempfile.TemporaryDirectory`; assert base/head use different checkout and `CARGO_TARGET_DIR`, identical environment/argv/image, stdout/stderr files per command, timeout maps to `inconclusive`, and base/head SHA is rechecked after execution.

Run: `python3 -m unittest tools.hwpx_loop.verifier_test -v`

Expected: FAIL because `run_verification` is absent.

- [ ] **Step 9: 최소 isolated runner와 output을 구현한다 (5분)**

Implement `run_verification(request, artifact_dir, run_command=subprocess.run)` to create `base/` and `head/` detached clean checkouts, run each profile command in the pinned container with separate target directories, preserve raw logs and duration, classify each command, emit schema version 1 canonical `verify.json`, and return the specified exit code. Never retry a failure into success; an explicit repeat mode may only report `non_deterministic`.

Expected GREEN: mocked runner tests PASS; output contains `schema_version`, `profile`, immutable SHAs, `verdict`, `commands`, `infrastructure`, and SHA-256 digest.

- [ ] **Step 10: CLI RED/GREEN 계약을 구현한다 (5분)**

Test then implement:

```bash
python3 -m tools.hwpx_loop.cli verify --request /tmp/request.json --artifacts /tmp/run
```

`cli_test.py` must assert invalid JSON/request exits 40, repair exits 20, objective-not-met exits 21, infrastructure exits 30, candidate exits 0, and stdout contains only canonical result JSON. Use `argparse`; do not add third-party packages.

- [ ] **Step 11: 전체 GREEN과 결정성을 확인하고 커밋한다 (5분 + 실행 대기)**

Run:

```bash
python3 -m unittest tools.hwpx_loop.verifier_test tools.hwpx_loop.cli_test -v
python3 -m tools.hwpx_loop.cli verify --request /tmp/p01-request.json --artifacts /tmp/p01-run-a
python3 -m tools.hwpx_loop.cli verify --request /tmp/p01-request.json --artifacts /tmp/p01-run-b
shasum -a 256 /tmp/p01-run-a/verify.json /tmp/p01-run-b/verify.json
git diff --check
git add tools/hwpx_loop/__init__.py tools/hwpx_loop/profile.py tools/hwpx_loop/verifier.py tools/hwpx_loop/verifier_test.py tools/hwpx_loop/cli.py tools/hwpx_loop/cli_test.py
git commit -m "feat: base head 차등 검증기 추가"
```

Expected: tests PASS; two digests identical; P0-1 target delta `improved`; no `new_regression`/`changed_failure`; commit contains only verifier files.

---

### Task 5: P0-4 Dependency Review 정책 적용

**Files:**
- Modify: `.github/workflows/dependency-review.yml` only if activation is approved.
- Modify: `packages/hwpx-python/scripts/check_workflows_test.py`
- Create/Modify: `docs/loop-engineering.md`

- [ ] **Step 1: 사람이 현재 기능 상태를 읽고 두 정책 중 하나를 승인한다 (3분)**

Run:

```bash
gh api repos/Han-taz/hwpx-rust --jq '{dependency_graph:.security_and_analysis.dependency_graph.status}'
```

Expected current observation: disabled/unavailable. Approver records exactly one choice against P0-4 and immutable head SHA: `activate` or `remain-disabled`.

- [ ] **Step 2: RED 정책 문서/검사 테스트를 작성한다 (4분)**

Add workflow test asserting action `actions/dependency-review-action@v5.0.0`, `fail-on-severity: moderate`, and `fail-on-scopes: runtime, unknown`. Add a document test (same test file may read `docs/loop-engineering.md`) asserting the approved choice, approver, approval UTC timestamp, target SHA, and exact phrase `infrastructure_unavailable is not a dependency safety pass`.

Run: `python3 packages/hwpx-python/scripts/check_workflows_test.py`

Expected: FAIL because `docs/loop-engineering.md` does not exist.

- [ ] **Step 3A: `activate`가 승인된 경우 사람이 설정을 바꾸고 PR check를 확인한다 (5분 + 대기)**

The human maintainer, not Hermes, runs:

```bash
gh api --method PATCH repos/Han-taz/hwpx-rust \
  -H 'Accept: application/vnd.github+json' \
  -f 'security_and_analysis[dependency_graph][status]=enabled'
gh api repos/Han-taz/hwpx-rust --jq .security_and_analysis.dependency_graph.status
```

Expected: `enabled`. Re-run Dependency Review on one approved PR and require success. Keep the workflow's pinned action/severity/scopes; add only a comment linking P0-4 if traceability is required.

- [ ] **Step 3B: `remain-disabled`가 승인된 경우 설정과 workflow를 바꾸지 않는다 (2분)**

Run:

```bash
git diff --exit-code -- .github/workflows/dependency-review.yml
```

Expected: PASS. Dependency Review is excluded from required gates and verifier records `infrastructure.status="infrastructure_unavailable"`; it never changes a code verdict to pass.

- [ ] **Step 4: 승인된 단일 정책을 문서화한다 (4분)**

Create `docs/loop-engineering.md` with: chosen policy, approver identity/time/SHA copied verbatim from the GitHub approval; Dependency Review infrastructure status separate from code deltas; required-gate effect; rollback; `gh api` verification command; no automatic merge; Hermes prohibition. Do not document both choices as active.

- [ ] **Step 5: GREEN과 커밋을 수행한다 (3분)**

Run:

```bash
python3 packages/hwpx-python/scripts/check_workflows_test.py
git diff --check
git add packages/hwpx-python/scripts/check_workflows_test.py docs/loop-engineering.md
git add .github/workflows/dependency-review.yml  # only for approved activate-path changes
git commit -m "docs: Dependency Review 운영 정책 확정"
```

Expected: PASS; commit reflects exactly the approved branch. GitHub setting itself is an external audited action, not a repository commit.

---

### Task 6: OpenCode 구현/리뷰 분리와 상태·감사 로그를 포함한 최소 orchestrator

**Files:**
- Create: `tools/hwpx_loop/orchestrator.py`
- Create: `tools/hwpx_loop/orchestrator_test.py`
- Modify: `tools/hwpx_loop/cli.py`
- Modify: `tools/hwpx_loop/cli_test.py`
- Modify: `docs/loop-engineering.md`

- [ ] **Step 1: RED 상태 머신과 이슈 계약 테스트를 작성한다 (5분)**

Encode allowed transitions exactly as the design (`proposed→ready→claimed→implementing→verifying→reviewing→awaiting-human→done`, repair/blocked/aborted branches). Test undefined transitions, terminal restart, and issue bodies missing any of the nine contract sections are rejected.

Run: `python3 -m unittest tools.hwpx_loop.orchestrator_test -v`

Expected: FAIL because `orchestrator.py` is absent.

- [ ] **Step 2: 최소 state/issue validation을 구현한다 (5분)**

Use frozen sets/dicts and return structured reason codes (`invalid_transition`, `incomplete_issue_contract`). Do not call GitHub or inspect code diff content in these functions.

Expected GREEN: state and issue contract tests PASS.

- [ ] **Step 3: RED handoff/역할 분리 테스트를 작성한다 (5분)**

Test required fields from schema version 1, exact model, full SHAs, command argv/exit code, allowed changed paths, commit existence callback, review findings shape. Reject same implementation/review `worker_id`, same `session_id`, review changed files, stale reviewed SHA, model mismatch, and third active worker.

Expected RED: missing `validate_handoff`/`validate_assignment`.

- [ ] **Step 4: handoff와 assignment validation을 구현한다 (5분)**

Implementation handoff result is only `ready_for_verification`; review result is only `approve` or `changes_requested`. Review findings require severity, file, line, evidence, required_action. Hermes stores/passes these fields unchanged and creates no summary judgment.

Expected GREEN: all role separation tests PASS.

- [ ] **Step 5: RED 잠금/lease 테스트를 작성한다 (5분)**

Using `TemporaryDirectory`, assert atomic `mkdir` permits one writer per repository+issue, lock JSON has the design fields, renewal updates lease, expiry alone cannot steal, stale recovery requires process/worktree/remote evidence and audit reason, dirty worktree is preserved.

Expected RED: lock functions absent.

- [ ] **Step 6: filesystem lock을 구현한다 (5분)**

Root defaults to `~/.local/state/hwpx-loop/locks`; use atomic directory creation and `lock.json` write-via-temp plus `os.replace`. Lease interval is 5 minutes. Provide explicit `inspect_stale_lock()` result but no automatic deletion.

Expected GREEN: lock tests PASS including concurrent second claimant rejection.

- [ ] **Step 7: RED hash-chain 감사 로그 테스트를 작성한다 (5분)**

Assert each canonical JSONL event includes event ID, previous hash, UTC timestamp, repository, issue/run/actor, old/new state, base/head SHA, action, reason, artifact digest, optional approval. Tampering with event 1 must make replay fail at event 2; `state.json` must rebuild from events.

Expected RED: event/replay functions absent.

- [ ] **Step 8: append-only event와 snapshot replay를 구현한다 (5분)**

Use `canonical_bytes`, SHA-256, append+flush+`os.fsync`; never rewrite `events.jsonl`. Write `state.json` atomically from replay. Mask values whose keys contain `token`, `secret`, or `authorization` before serialization. Retention metadata is 30 days success, 90 days failure/abort.

Expected GREEN: chain, tamper, replay, masking tests PASS.

- [ ] **Step 9: RED OpenCode command builder 테스트를 작성한다 (4분)**

Assert command builder uses exact model `openai/gpt-5.6-sol`, unique session/worker IDs, one issue/worktree, implementation writable allowed paths, review detached/read-only worktree, and rejects active worker count >2. Because `opencode` is not installed in the planning environment, tests mock process launch.

Expected RED: command builder absent.

- [ ] **Step 10: command builder와 CLI를 구현한다 (5분)**

Build argv as data, never `shell=True`. CLI subcommands only validate, transition, or print worker argv/request JSON. Hermes does not generate implementation/review prose. Review request references exact head SHA and original issue acceptance criteria.

Expected GREEN: CLI tests show implementation and review have different `--session` and identical `--model openai/gpt-5.6-sol`.

- [ ] **Step 11: repair/중단 정책 테스트와 구현을 추가한다 (5분)**

Test verifier 20 or review `changes_requested` enters repair; new commit invalidates prior review; third repair enters `awaiting-human`; model/path/double-lock/worker-limit/SHA mutation enters `aborted` and preserves evidence; infrastructure 30 enters `blocked`.

Expected GREEN: no undefined automatic state transition and no verifier status override.

- [ ] **Step 12: 전체 GREEN과 운영 문서를 확인하고 커밋한다 (5분)**

Run:

```bash
python3 -m unittest tools.hwpx_loop.verifier_test tools.hwpx_loop.orchestrator_test tools.hwpx_loop.cli_test -v
python3 -m tools.hwpx_loop.cli validate-issue --input /tmp/p01-issue.json
python3 -m tools.hwpx_loop.cli validate-handoff --input /tmp/p01-handoff.json
git diff --check
git add tools/hwpx_loop/orchestrator.py tools/hwpx_loop/orchestrator_test.py tools/hwpx_loop/cli.py tools/hwpx_loop/cli_test.py docs/loop-engineering.md
git commit -m "feat: 역할 분리 오케스트레이터 추가"
```

Expected: all tests PASS; state artifacts exist only under external state root during real runs; repository contains no token/log/run output.

---

### Task 7: `#11` 또는 `#12` 제한 파일럿

**Files:**
- Repository changes: none unless the selected existing Dependabot branch itself already contains dependency files.
- External state: GitHub issue/PR labels/comments, `~/.local/state/hwpx-loop/` run artifacts and worktrees.

- [ ] **Step 1: 두 PR을 최신 사실로 비교하고 사람이 정확히 하나를 선택한다 (4분)**

Run:

```bash
gh pr view 11 --repo Han-taz/hwpx-rust --json number,title,baseRefOid,headRefOid,mergeable,files,statusCheckRollup
gh pr view 12 --repo Han-taz/hwpx-rust --json number,title,baseRefOid,headRefOid,mergeable,files,statusCheckRollup
```

Expected: `#11`은 cfb, `#12`는 criterion 변경. 사람이 충돌/변경 범위가 더 작은 하나와 immutable base/head SHA를 승인한다. 자동 선택하지 않는다.

- [ ] **Step 2: 최신 main으로 갱신할 별도 승인과 파일 허용 목록을 기록한다 (3분)**

Expected: 승인에는 selected PR, latest `origin/main` SHA, dependency manifest/lockfile의 기존 PR 변경 경로, repair 2회, 45분, no-auto-merge가 포함된다. 범위가 달라지면 중단한다.

- [ ] **Step 3: 이슈 잠금과 detached 검증 입력을 만든다 (4분)**

Run orchestrator `validate-issue`, acquire one writer lock, create run ID, and write request JSON. Ensure active OpenCode workers <=2 and no verifier for this repository is already active.

Expected: state `ready→claimed`; request includes approved profile, image digest, exact SHAs, network/cache policy.

- [ ] **Step 4: 구현 워커가 rebase/update와 기계 검증만 수행한다 (5분 + 대기)**

Start a new OpenCode implementation session using `openai/gpt-5.6-sol`. It may update the selected Dependabot branch to approved latest main and resolve only approved dependency-file conflicts; it must not merge or broaden dependency changes.

RED: run `baseline-v1` against approved base and original head; expected current baseline failures are classified, not treated as PR regressions.

GREEN: run against approved base and updated head; expected no `new_regression`/`changed_failure`, and selected dependency objective remains present.

- [ ] **Step 5: deterministic verifier 판정을 확인한다 (5분 + 실행 대기)**

Run:

```bash
RUN_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["run_id"])' /tmp/selected-pilot-request.json)
RUN_DIR="$HOME/.local/state/hwpx-loop/runs/$RUN_ID"
python3 -m tools.hwpx_loop.cli verify --request "$RUN_DIR/request.json" --artifacts "$RUN_DIR"
```

Expected: exit 0 `candidate_pass`. Dependency Review disabled policy, if selected, appears only as `infrastructure_unavailable`. Exit 20 goes to repair, 30 to blocked, 40 to aborted/invalid run; none may be manually changed to pass.

- [ ] **Step 6: 별도 review worktree와 OpenCode 리뷰 세션을 실행한다 (5분 + 대기)**

Read `issue` and `head_sha` from `$RUN_DIR/request.json`, then create `issue-${ISSUE}-review-${HEAD_SHA:0:8}` detached at that exact head and apply read-only policy. Start a different OpenCode session/worker ID with model `openai/gpt-5.6-sol`; provide issue criteria, verifier JSON, and diff read access. Reviewer returns only structured `approve` or `changes_requested` and does not edit files.

```bash
ISSUE=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["issue"])' "$RUN_DIR/request.json")
HEAD_SHA=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["head_sha"])' "$RUN_DIR/request.json")
git worktree add --detach "../issue-${ISSUE}-review-${HEAD_SHA:0:8}" "$HEAD_SHA"
```

Expected: handoff schema validates; same worker/session or changed SHA is rejected.

- [ ] **Step 7: repair가 필요하면 최대 2회 verification부터 반복한다 (3분 조정 + 실행 대기)**

Pass verifier JSON or review findings verbatim to implementation worker. Each repair creates a commit; discard previous review; repeat verifier then a fresh review session. Third repair request transitions to `awaiting-human` and stops.

- [ ] **Step 8: 감사 로그와 사람 승인 지점을 검증한다 (4분)**

Run CLI replay/validate commands and verify event hash chain, lock ownership, model/session separation, exact SHA, command logs/digests, approvals, and no secret. Remove review worktree after recording result; preserve implementation worktree until human decision.

Expected: all events replay to current state; worker count never exceeded 2; Hermes authored zero code/review content.

- [ ] **Step 9: 사람에게 최종 결정을 요청하고 자동 병합 없이 종료한다 (3분)**

On approve + candidate_pass, transition to `loop:awaiting-human`. A human may merge/close or request repair. Do not run `gh pr merge`, do not enable auto-merge, and do not advance to the other PR automatically.

Expected: one limited pilot completed or explicitly stopped with evidence; expansion requires a new human approval.

---

## Final Verification

- [ ] **Step 1: repository verification을 실행한다 (5분 + 실행 대기)**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test -p hwpx-python --no-default-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p hwpx-python --no-default-features --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --locked --no-deps
python3 packages/hwpx-python/scripts/check_workflows_test.py
python3 -m unittest tools.hwpx_loop.verifier_test tools.hwpx_loop.orchestrator_test tools.hwpx_loop.cli_test -v
```

Expected: all PASS.

- [ ] **Step 2: 범위와 운영 불변식을 확인한다 (4분)**

Run:

```bash
git diff --check main...HEAD
git diff --name-only main...HEAD
```

Expected: only files enumerated in this plan; no run logs, state, tokens, unapproved lockfile/manifests, fuzz target deletion, lint allow, or auto-merge configuration.

- [ ] **Step 3: MVP 완료 증거를 사람이 확인한다 (5분)**

Expected evidence: P0-1/P0-2 separate implementation and review sessions; verifier canonical digest repeatability; P0-4 approval; event replay; no role/path/lock/worker-limit violation; selected `#11` or `#12` only; no automatic merge. Missing evidence keeps state `awaiting-human` or `blocked`, never `done`.
