# hwpx-rust 루프 엔지니어링 파일럿 설계

## 1. 문서 상태와 결정

- 작성일: 2026-07-16
- 대상 저장소: `Han-taz/hwpx-rust`
- 대상 기본 브랜치: `main`
- 설계 단계: 기준선 복구 우선 MVP와 두 개 파일럿
- 최종 결정: Hermes가 업무를 조정하고, 서로 분리된 OpenCode 구현 워커와 리뷰 워커가 작업하며, 동일 시점의 base/head를 결정적으로 검증하는 이슈 중심 루프를 채택한다.

이 문서는 자동 구현 플랫폼 전체가 아니라, 현재 깨진 기준선을 먼저 복구하고 작은 범위에서 루프의 안전성과 유효성을 증명하기 위한 운영 설계다. 구현 착수 여부와 병합은 사람이 결정한다.

## 2. 현재 사실과 출발 조건

2026-07-16 조회 결과를 설계의 기준으로 삼는다.

- 열린 일반 이슈는 0개다. 따라서 파일럿 업무는 먼저 사람이 승인한 이슈로 명시해야 한다.
- Dependabot PR `#8`부터 `#13`까지 여섯 개가 열려 있다.
- 현재 `main` 커밋 `6d1371c`의 최신 CI 실행에서도 `Clippy`와 `Fuzz Target Build`가 실패한다.
- `Clippy`는 `.github/workflows/ci.yml`의 `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`에서 실패한다.
- `Fuzz Target Build`는 `nightly`와 실행 시점에 설치되는 `cargo-fuzz`를 사용한다. 최신 `main` 실행에서는 `cargo-fuzz v0.13.1` 설치 중 전이 의존성 `rustix v0.36.5`와 당시 nightly Rust의 비호환으로 실패했다.
- PR `#13`의 fuzz 실패는 설치 이후 `Check fuzz workspace`에서 발생했다. 같은 체크 이름의 실패라도 원인이 다를 수 있다.
- Dependency Review 워크플로 파일은 활성 상태지만 저장소의 Dependency graph가 비활성화되어 모든 PR에서 실질적으로 사용할 수 없다. 이 문서에서 "Dependency Review 비활성화"는 워크플로 파일 부재가 아니라 기반 저장소 기능 비활성으로 검사가 수행 불가능한 상태를 뜻한다.
- `main` 브랜치 보호는 설정되지 않았고, ruleset은 비활성화되어 필수 상태 검사가 강제되지 않는다.
- PR `#8`, `#11`, `#13`의 base는 `6d1371c`이고, PR `#9`, `#10`, `#12`의 base는 더 오래된 `3959dfa`다. 오래된 base 결과와 현재 head 결과의 단순 비교는 도구 체인과 advisory DB의 시간 차이 때문에 신뢰할 수 없다.
- `main`의 기존 `Clippy`와 fuzz 실패를 head의 신규 회귀로 판정해서는 안 된다. 같은 시점, 같은 실행 환경에서 base와 head를 다시 실행해 차이를 판정해야 한다.

현재 PR별 관찰은 다음과 같다.

| PR | 변경 | 기존 기준선 실패 외 관찰된 실패 |
|---|---|---|
| `#8` | Rust minor/patch 그룹 | Dependency Review 인프라 실패 |
| `#9` | `quick-xml` 0.37.5 → 0.40.1 | Test, Coverage, Rustdoc, Benchmark Compile, Python Package |
| `#10` | `zip` 2.4.2 → 8.6.0 | Test, Coverage |
| `#11` | `cfb` 0.12.1 → 0.14.0 | 유효한 신규 실패가 아직 확인되지 않음 |
| `#12` | `criterion` 0.5.1 → 0.8.2 | 유효한 신규 실패가 아직 확인되지 않음 |
| `#13` | `actions/checkout` 6 → 7 | Security Audit, Dependency Policy |

이 표는 업무 우선순위를 위한 관찰값이며 병합 판정이 아니다. 모든 PR은 deterministic verifier로 다시 판정한다.

## 3. 목적

MVP의 목적은 다음 다섯 가지다.

1. `main`의 Clippy와 fuzz 실패를 독립 이슈로 복구해 신뢰할 수 있는 기준선을 만든다.
2. Hermes가 업무 분할, 배정, 상태 전이, 검증 실행 조정만 수행하고 코드 작성과 코드 리뷰에 관여하지 않는 경계를 검증한다.
3. 모든 구현과 코드 리뷰를 `openai/gpt-5.6-sol`을 사용하는 서로 분리된 OpenCode 워커에게 맡긴다.
4. base/head를 동일 환경에서 비교해 기존 실패, 신규 회귀, 개선, 인프라 실패를 재현 가능하게 구분한다.
5. 동시 워커를 최대 2개로 제한한 상태에서 이슈별 격리, 잠금, repair loop, 사람 승인 지점, 감사 가능성을 증명한다.

## 4. 비목표

- 열린 Dependabot PR 여섯 개를 MVP에서 자동 병합하지 않는다.
- Hermes가 소스 코드, 테스트, CI, 설정, 의존성 파일을 직접 수정하지 않는다.
- Hermes가 diff를 읽고 코드 품질을 판단하거나 코드 리뷰 의견을 작성하지 않는다.
- 구현 워커가 자신의 변경을 최종 리뷰하지 않는다.
- 리뷰 워커가 리뷰 중 발견한 문제를 직접 수정하지 않는다.
- GitHub 저장소 설정, Dependency graph, 브랜치 보호, ruleset을 사람 승인 없이 변경하지 않는다.
- 범용 멀티 저장소 플랫폼, 웹 대시보드, 장기 실행 서비스, 자동 병합 봇을 만들지 않는다.
- 실패한 기존 기준선을 허용 목록으로 영구 고정하지 않는다.
- HWP 5.0 기능 개발이나 HWPX 파서 기능 확장을 파일럿 범위에 넣지 않는다.

## 5. 설계 원칙

1. **기준선 우선:** 신규 기능보다 `main` 복구가 먼저다.
2. **역할 분리:** 조정, 구현, 리뷰, 승인 권한을 한 주체에 합치지 않는다.
3. **이슈가 작업 단위:** 하나의 승인된 이슈는 하나의 브랜치, worktree, 잠금, 상태 기록을 갖는다.
4. **동일 조건 비교:** 저장된 과거 체크가 아니라 같은 verifier 실행에서 base와 head를 비교한다.
5. **결정적 결과:** 동일 입력과 고정된 도구 체인은 동일한 판정 JSON과 종료 코드를 만든다.
6. **최소 권한:** 각 역할은 필요한 읽기·쓰기 권한만 갖는다.
7. **명시적 중단:** 불확실한 상태를 성공으로 간주하지 않고 사람 승인 또는 중단으로 전이한다.
8. **감사 가능성:** 배정, 명령, 결과, 상태 전이, 승인 주체를 append-only 로그로 남긴다.

## 6. 역할과 권한

### 6.1 Hermes: PM/오케스트레이터

Hermes가 할 수 있는 일:

- 승인된 목표를 이슈 단위로 분할하고 의존 관계와 우선순위를 정한다.
- GitHub 이슈의 라벨과 상태를 읽고, 정책이 허용하는 상태 전이와 배정 기록을 남긴다.
- 이슈별 worktree와 잠금을 생성·회수하도록 실행기를 호출한다.
- OpenCode 워커를 `openai/gpt-5.6-sol`로 시작하고 역할별 프롬프트와 허용 경로를 전달한다.
- deterministic verifier를 호출하고 구조화된 결과의 상태 코드만 해석한다.
- 구현 워커와 리뷰 워커의 결과물을 다음 단계에 전달한다.
- 재시도 횟수, 시간, 비용, 병렬성 한도를 적용한다.
- 사람 승인이 필요한 상태를 표시하고 대기한다.

Hermes가 해서는 안 되는 일:

- 코드, 테스트, CI, 설정, 의존성, 문서 구현물을 직접 작성하거나 수정한다.
- 구현 diff를 품질 관점에서 직접 판단한다.
- 코드 리뷰를 수행하거나 승인·변경 요청 의견을 스스로 생성한다.
- verifier의 실패를 임의로 무시하거나 성공으로 바꾼다.
- PR을 병합하거나 GitHub 저장소 설정을 변경한다.

Hermes는 기계 판정의 전달자이지 리뷰어가 아니다. 예를 들어 verifier가 `new_regression`을 반환하면 해당 상태를 repair로 전이할 수 있지만, 어떤 코드를 어떻게 고칠지는 구현 워커가 결정한다.

### 6.2 구현 워커

- 실행 환경은 OpenCode, 모델은 정확히 `openai/gpt-5.6-sol`이다.
- 한 번에 하나의 이슈와 하나의 전용 worktree만 소유한다.
- 이슈 허용 범위 안에서 코드, 테스트, CI, 설정 또는 의존성을 수정할 수 있다.
- 작업 전 기준선 재현, 최소 변경, 관련 테스트 추가, 로컬 검증, 커밋을 담당한다.
- 자신의 변경에 대한 최종 코드 리뷰나 병합 승인을 하지 않는다.
- 검증 결과와 변경 요약을 구조화된 handoff로 남긴다.

### 6.3 리뷰 워커

- 실행 환경은 OpenCode, 모델은 정확히 `openai/gpt-5.6-sol`이다.
- 구현 워커와 다른 OpenCode 세션이며 별도 worker ID를 갖는다.
- 구현 worktree를 읽기 전용으로 보거나, 같은 커밋을 별도 review worktree에서 checkout한다.
- 요구사항 누락, 버그, 회귀, 보안 위험, 테스트 공백을 검토하고 `approve` 또는 `changes_requested`를 구조화해 반환한다.
- 어떤 파일도 수정하거나 커밋하지 않는다.
- Hermes 대신 상태를 바꾸거나 병합하지 않는다.

### 6.4 사람 유지관리자

- 파일럿 시작과 이슈 범위를 승인한다.
- GitHub 설정 변경과 외부 보안 정책 변경을 승인하고 직접 적용하거나 별도 승인 작업으로 위임한다.
- verifier 예외, 범위 확대, repair 한도 초과를 판단한다.
- 리뷰 승인과 verifier 통과를 확인한 후 최종 병합 또는 종료를 결정한다.

## 7. GitHub 라벨과 상태 머신

### 7.1 라벨 집합

MVP는 다음 라벨만 사용한다.

| 라벨 | 의미 |
|---|---|
| `loop:proposed` | 후보로 작성되었으나 사람이 아직 승인하지 않음 |
| `loop:ready` | 범위와 수용 기준이 승인되어 배정 가능 |
| `loop:claimed` | worktree와 잠금이 확보됨 |
| `loop:implementing` | 구현 워커가 작업 중 |
| `loop:verifying` | deterministic verifier 실행 중 |
| `loop:reviewing` | 분리된 리뷰 워커가 검토 중 |
| `loop:repair` | 검증 또는 리뷰 지적을 구현 워커가 수정 중 |
| `loop:awaiting-human` | 사람 결정 없이는 진행할 수 없음 |
| `loop:blocked` | 외부 의존성 또는 인프라로 진행 불가 |
| `loop:done` | 사람이 병합 또는 완료 처리함 |
| `loop:aborted` | 정책에 따라 중단하고 자원을 회수함 |
| `priority:p0` | 기준선과 안전 장치를 막는 최우선 작업 |
| `kind:baseline` | `main` 기준선 복구 |
| `kind:dependency` | 의존성 또는 Dependabot PR 처리 |
| `kind:infrastructure` | CI, verifier, GitHub 기능 관련 작업 |

한 이슈에는 `loop:*` 라벨을 정확히 하나만 둔다. 우선순위와 종류 라벨은 상태와 독립적이다.

### 7.2 허용 상태 전이

```text
proposed --사람 승인--> ready
ready --lock 획득--> claimed
claimed --구현 워커 시작--> implementing
implementing --handoff 유효--> verifying
verifying --head 신규 실패 없음--> reviewing
verifying --head 신규 실패/명령 실패--> repair
verifying --인프라 불확실--> blocked
reviewing --approve--> awaiting-human
reviewing --changes_requested--> repair
repair --수정 handoff--> verifying
awaiting-human --병합/완료--> done
awaiting-human --수정 요구--> repair
blocked --원인 해소 및 사람 재개--> ready
모든 비종료 상태 --중단 조건 충족--> aborted
```

Hermes는 정의되지 않은 전이를 거부한다. `done`과 `aborted`는 종료 상태이며 재개하려면 사람이 새 이슈를 만들거나 종료 이슈를 명시적으로 다시 열어 `ready`로 승인해야 한다.

## 8. 이슈 계약

`loop:ready`가 되기 전에 이슈 본문에는 다음 항목이 모두 있어야 한다.

- 문제와 사용자 영향
- 재현 가능한 base 실패 명령 및 현재 관찰
- 허용 변경 경로와 금지 변경 경로
- 수용 기준
- deterministic verifier 프로필
- 선행 이슈와 충돌 가능 영역
- 사람 승인이 필요한 변경 유형
- 최대 repair 횟수와 최대 실행 시간
- 롤백 방법

Hermes는 빠진 필드의 기술 내용을 추측하지 않는다. 누락된 이슈는 `loop:proposed`에 유지하고 사람에게 보완을 요청한다.

## 9. 이슈별 worktree와 잠금

### 9.1 이름과 위치

- 브랜치: `loop/issue-<번호>-<slug>`
- 구현 worktree: 저장소 공용 worktree 루트 아래 `issue-<번호>-impl`
- 리뷰 worktree: `issue-<번호>-review-<head-short-sha>`
- 런 ID: `YYYYMMDDTHHMMSSZ-issue-<번호>-<8자리 난수>`

리뷰 worktree는 구현 브랜치를 수정하지 못하도록 detached HEAD와 읽기 전용 정책으로 연다. 리뷰가 끝나면 즉시 제거한다.

### 9.2 잠금

잠금 키는 저장소 ID와 이슈 번호의 조합이다. 잠금 레코드는 저장소 밖의 상태 디렉터리 `~/.local/state/hwpx-loop/locks/`에 둔다.

```json
{
  "repository": "Han-taz/hwpx-rust",
  "issue": 101,
  "run_id": "20260716T120000Z-issue-101-a1b2c3d4",
  "worker_id": "impl-01",
  "role": "implementation",
  "worktree": "/absolute/path/issue-101-impl",
  "base_sha": "6d1371c704853e6b427296b98fc9da4d0c5e49c6",
  "acquired_at": "2026-07-16T12:00:00Z",
  "lease_expires_at": "2026-07-16T12:30:00Z"
}
```

- 원자적 디렉터리 생성 또는 OS 파일 잠금으로 한 이슈에 하나의 쓰기 소유자만 허용한다.
- 워커는 5분마다 lease를 갱신한다.
- lease 만료만으로 다른 워커가 즉시 소유권을 빼앗지 않는다. Hermes가 프로세스 부재, worktree 상태, 원격 브랜치 상태를 확인하고 stale 판정을 로그에 남긴 뒤 회수한다.
- 구현 잠금이 살아 있는 동안 두 번째 구현 워커를 배정하지 않는다.
- 리뷰 잠금은 읽기 전용이며 구현 잠금과 공존할 수 있지만, 리뷰 대상 SHA가 바뀌면 해당 리뷰 결과를 폐기한다.
- 잠금 해제 전에 미커밋 파일, 커밋 SHA, verifier 결과, handoff 저장 여부를 확인한다.

## 10. 워커 handoff 계약

모든 워커는 자유 형식 설명과 함께 다음 JSON 필드를 반환한다.

```json
{
  "schema_version": 1,
  "run_id": "20260716T120000Z-issue-101-a1b2c3d4",
  "issue": 101,
  "worker_id": "impl-01",
  "role": "implementation",
  "model": "openai/gpt-5.6-sol",
  "base_sha": "6d1371c704853e6b427296b98fc9da4d0c5e49c6",
  "head_sha": "0123456789abcdef0123456789abcdef01234567",
  "changed_files": ["path/to/file"],
  "commands": [{"argv": ["cargo", "test", "--workspace", "--locked"], "exit_code": 0}],
  "result": "ready_for_verification",
  "summary": "변경과 남은 위험 요약"
}
```

리뷰 handoff의 `role`은 `review`, `result`는 `approve` 또는 `changes_requested`이며, 발견 사항마다 심각도, 파일, 줄, 근거, 요구 조치를 포함한다. `worker_id`가 구현 handoff와 같거나 모델이 다르면 Hermes는 결과를 무효 처리한다.

## 11. 구현·검증·리뷰·repair loop

1. Hermes가 `loop:ready` 중 의존성이 없는 P0 이슈를 선택한다.
2. Hermes가 worktree와 구현 잠금을 확보하고 이슈를 `claimed`로 전이한다.
3. 구현 워커가 base 실패를 재현하고, 수용 기준을 만족하는 최소 변경과 회귀 테스트를 작성해 커밋한다.
4. Hermes는 handoff 스키마, worker ID, 모델명, 허용 경로, 커밋 존재만 기계적으로 검사한다. 코드 내용은 평가하지 않는다.
5. deterministic verifier가 동일한 도구 이미지와 명령 집합으로 base와 head를 실행한다.
6. 신규 실패가 없고 목표 체크가 개선되면 별도 리뷰 워커를 시작한다.
7. 리뷰 워커가 `approve`를 반환하면 Hermes는 `awaiting-human`으로 전이한다.
8. verifier가 신규 실패를 반환하거나 리뷰 워커가 `changes_requested`를 반환하면 Hermes는 `repair`로 전이하고 구현 워커에 구조화된 결과를 전달한다.
9. 구현 워커가 새 커밋을 만들면 이전 리뷰 결과를 폐기하고 verification부터 다시 시작한다.
10. repair는 기본 최대 2회다. 세 번째 수정이 필요하면 자동 진행을 멈추고 사람에게 범위 재설계, 워커 교체, 중단 중 하나를 요청한다.

구현과 리뷰는 항상 다른 OpenCode 워커가 수행한다. Hermes는 리뷰 내용을 요약해 새 판단을 만들지 않고 원문과 상태 코드만 전달한다.

## 12. Deterministic verifier

### 12.1 목적과 입력

verifier는 코드 리뷰를 대신하지 않는다. base와 head의 기계적 품질 신호를 같은 조건에서 비교한다.

필수 입력:

- repository와 issue 번호
- 불변의 `base_sha`와 `head_sha`
- verifier profile 버전
- 고정된 컨테이너 이미지 digest
- Rust stable/nightly의 정확한 날짜 또는 버전
- 설치 도구의 정확한 버전
- 명령별 timeout
- 네트워크 정책과 캐시 정책

`stable`, `nightly`, `latest`처럼 실행마다 바뀌는 값은 판정 입력으로 허용하지 않는다. advisory DB를 사용하는 검사에는 DB revision 또는 snapshot digest를 기록한다.

### 12.2 실행 규칙

- 깨끗한 임시 checkout에서 base와 head를 각각 실행한다.
- 두 실행은 같은 이미지, 환경 변수, lockfile 정책, 도구 버전, CPU·메모리 한도, 네트워크 정책을 사용한다.
- 상태 오염을 막기 위해 target 디렉터리는 공유하지 않는다. 다운로드 캐시는 읽기 전용 content-addressed cache만 허용한다.
- 명령 순서는 profile에 고정한다.
- stdout/stderr 원문, 종료 코드, 소요 시간, 도구 버전, 산출물 digest를 보존한다.
- flaky 재실행은 자동 성공 변환에 사용하지 않는다. 동일 입력으로 한 번 더 실행해 `non_deterministic`을 확인하는 용도로만 사용한다.

기준 profile의 명령은 저장소 CI와 일치시킨다.

```text
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test -p hwpx-python --no-default-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p hwpx-python --no-default-features --all-targets --locked -- -D warnings
RUSTDOCFLAGS=-D warnings cargo doc --workspace --all-features --locked --no-deps
cargo bench -p hwp-core --bench parse_benchmark --locked -- --test
cargo check --manifest-path fuzz/Cargo.toml --locked
cargo fuzz list
cargo fuzz build parse_auto
cargo fuzz build parse_hwpx
```

보안 profile은 고정 advisory DB snapshot을 사용해 `cargo audit`과 `cargo deny check --locked`를 별도로 실행한다. Dependency Review는 base push에 대칭 체크가 없고 현재 Dependency graph도 비활성화되어 있으므로 verifier의 코드 회귀 판정에서 제외하고 `infrastructure_unavailable`로 기록한다.

### 12.3 차등 판정

명령별 base/head 결과를 다음과 같이 판정한다.

| Base | Head | 판정 |
|---|---|---|
| 통과 | 통과 | `unchanged_pass` |
| 실패 | 통과 | `improved` |
| 실패 | 같은 정규화 원인으로 실패 | `pre_existing_failure` |
| 실패 | 다른 정규화 원인으로 실패 | `changed_failure` |
| 통과 | 실패 | `new_regression` |
| 어느 한쪽이 timeout, 취소, 인프라 오류 | 비교 불가 | `inconclusive` |

정규화 원인은 체크 이름만으로 만들지 않는다. 명령 argv, 실패 단계, 종료 코드, 컴파일러 diagnostic code, 실패 테스트 이름, 오류 메시지에서 경로·시간·임시 ID를 제거한 fingerprint를 사용한다. 예를 들어 `Install cargo-fuzz` 실패와 `Check fuzz workspace` 실패는 둘 다 `Fuzz Target Build`여도 다른 원인이다.

전체 판정 규칙:

- `new_regression` 또는 `changed_failure`가 하나라도 있으면 종료 코드 `20`, 결과 `repair_required`다.
- 목표 실패가 `improved`이고 신규·변경 실패가 없으면 종료 코드 `0`, 결과 `candidate_pass`다.
- 목표 실패가 그대로면 종료 코드 `21`, 결과 `objective_not_met`이다.
- `inconclusive`가 하나라도 있으면 종료 코드 `30`, 결과 `infrastructure_blocked`다.
- 입력 불일치, dirty checkout, toolchain digest 불일치는 실행 전 종료 코드 `40`, 결과 `invalid_run`이다.

### 12.4 출력

출력은 canonical JSON으로 직렬화한다. 키 순서, UTF-8, 줄바꿈, timestamp 형식을 고정하고 결과 파일의 SHA-256을 감사 로그에 남긴다.

```json
{
  "schema_version": 1,
  "profile": "baseline-v1",
  "base_sha": "6d1371c704853e6b427296b98fc9da4d0c5e49c6",
  "head_sha": "0123456789abcdef0123456789abcdef01234567",
  "verdict": "candidate_pass",
  "commands": [
    {"id": "clippy-workspace", "base": "fail", "head": "pass", "delta": "improved"}
  ]
}
```

## 13. 병렬성 및 스케줄링

- 동시에 실행되는 OpenCode 워커는 구현과 리뷰를 합쳐 최대 2개다.
- verifier 프로세스는 워커 수에 포함하지 않지만, 저장소별 verifier는 한 번에 하나만 실행해 캐시·리소스 경쟁을 막는다.
- 같은 이슈에는 구현 워커 1개와 리뷰 워커 1개만 존재할 수 있다.
- 같은 파일 집합을 수정할 가능성이 높은 두 구현 이슈는 병렬 실행하지 않는다.
- P0 기준선 이슈가 존재하면 dependency 이슈보다 먼저 배정한다.
- 기본 스케줄은 구현 1개와 리뷰 1개다. 리뷰 대기 작업이 없고 두 이슈가 경로상 독립적일 때만 구현 2개를 병렬 실행한다.
- 리뷰 워커 슬롯이 없으면 검증을 통과한 이슈는 `loop:verifying` 결과를 보존한 채 대기하며 새 구현을 무제한 시작하지 않는다.

## 14. 사람 승인 지점

다음 단계는 반드시 사람의 명시적 승인이 필요하다.

1. 후보 이슈를 `loop:proposed`에서 `loop:ready`로 전환할 때
2. 허용 변경 경로를 넓히거나 수용 기준을 바꿀 때
3. CI, 저장소 설정, Dependency graph, 브랜치 보호, ruleset, secret, 권한을 변경할 때
4. lockfile 또는 직접 의존성 변경이 원래 이슈 범위에 없을 때
5. verifier의 `inconclusive` 또는 예외 허용을 요청할 때
6. repair 2회 한도를 초과할 때
7. 리뷰 승인과 verifier 통과 후 브랜치를 병합할 때
8. 자동화 파일럿을 다음 단계로 확대할 때

Hermes는 승인자의 GitHub identity, 승인 시각, 승인 대상 SHA와 범위를 감사 로그에 기록한다. 구두 승인이나 SHA가 없는 승인은 유효하지 않다.

## 15. 실패, 복구, 중단 조건

### 15.1 복구 가능한 실패

- 워커 프로세스 종료: 마지막 커밋과 handoff를 확인하고 같은 역할의 새 워커를 배정한다.
- lease 만료: stale 확인 후 잠금을 회수하고 dirty worktree를 보존한 채 사람에게 알린다.
- verifier 신규 회귀: `repair`로 전이하고 diff가 아니라 구조화된 실패 결과를 구현 워커에 전달한다.
- 리뷰 변경 요청: `repair`로 전이하고 리뷰 원문을 구현 워커에 전달한다.
- GitHub API 일시 오류: 지수 backoff로 3회 재시도하고, 계속 실패하면 로컬 상태를 보존한 채 `blocked`로 전이한다.
- base가 이동함: 기존 실행을 폐기하지 않고 `superseded`로 기록한 뒤 새 base SHA로 사람 재승인을 받는다.

### 15.2 즉시 중단 조건

- 워커 모델이 `openai/gpt-5.6-sol`이 아님
- 구현자와 리뷰어 worker ID 또는 세션이 같음
- Hermes가 코드 또는 리뷰 결과를 작성한 흔적이 발견됨
- 허용 경로 밖 파일 변경, secret 접근, 강제 push, history rewrite 시도
- verifier 입력 digest 또는 결과 로그 변조
- 동일 이슈의 이중 쓰기 잠금
- 총 워커 수 2개 초과
- base/head SHA가 검증 중 변경됨
- 사람이 중단을 요청함

즉시 중단 시 해당 이슈를 `loop:aborted`로 전이하고 프로세스를 종료하며 잠금과 worktree를 보존 모드로 둔다. 증거 수집 전에 자동 삭제하거나 재시작하지 않는다.

### 15.3 예산 중단 조건

- 구현 또는 repair 단일 시도 45분 초과
- verifier 단일 명령이 profile timeout 초과
- repair 2회 초과
- 같은 fingerprint의 비결정적 실패가 2회 연속 발생
- 이슈 생성 후 2영업일 동안 사람 승인이 없음

예산 중단은 `awaiting-human` 또는 `blocked`로 전이하며 실패 원인과 사용량을 기록한다.

## 16. 상태 저장과 감사 로그

MVP의 실행 상태는 저장소 밖 `~/.local/state/hwpx-loop/`에 둔다. 생성된 코드 변경과 분리해 오케스트레이터 장애 시에도 복구할 수 있게 한다.

```text
state.json                 현재 큐와 워커 lease의 원자적 snapshot
events.jsonl               append-only 상태 전이 이벤트
locks/                     이슈별 잠금
runs/<run-id>/request.json 워커 입력
runs/<run-id>/handoff.json 워커 출력
runs/<run-id>/verify.json  canonical verifier 결과
runs/<run-id>/logs/        명령별 stdout/stderr
```

각 이벤트에는 다음 필드를 둔다.

- event ID와 이전 event hash
- UTC timestamp
- repository, issue, run ID
- actor type과 actor ID
- 이전 상태와 새 상태
- base SHA와 head SHA
- action, reason code, 관련 artifact digest
- 사람 승인인 경우 approver와 승인 범위

`events.jsonl`은 이전 이벤트 hash를 포함하는 hash chain으로 변조를 탐지한다. `state.json`은 이벤트를 재생해 재구성할 수 있어야 하며 단독 진실 원천으로 사용하지 않는다. GitHub 이슈에는 민감하지 않은 상태 요약과 run ID만 기록하고, 전체 로그나 환경 정보는 게시하지 않는다.

보존 기간은 성공 실행 30일, 실패·중단 실행 90일이다. secret과 GitHub token은 로그 전에 마스킹하고 원문을 저장하지 않는다.

## 17. P0 기준선 복구 이슈 후보

### P0-1. `main` Clippy 기준선 복구

- 종류: `priority:p0`, `kind:baseline`
- 문제: 현재 `main`의 workspace/all-targets/all-features Clippy가 `-D warnings`에서 실패한다.
- 범위: 실제 diagnostic이 가리키는 Rust 소스와 필요한 회귀 테스트만 허용한다.
- 금지: Clippy 경고 전역 허용, `-D warnings` 제거, 관련 없는 리팩터링.
- 수용 기준: 동일 profile에서 base `pre_existing_failure`, head `improved`; 나머지 명령에 신규·변경 실패 없음.
- 사람 승인: 수정이 공개 API, 직렬화 형식, Python API에 영향을 주면 repair 전에 승인.

### P0-2. fuzz 도구 체인과 target build 기준선 복구

- 종류: `priority:p0`, `kind:baseline`, `kind:infrastructure`
- 문제: 실행 시점의 nightly와 `cargo install cargo-fuzz --locked`가 비결정적이며, 설치 실패와 workspace check 실패가 같은 job 이름으로 섞인다.
- 범위: cargo-fuzz 버전과 nightly 고정, fuzz workspace check 및 두 target build 재현성 확보에 필요한 최소 CI/fuzz 변경.
- 금지: fuzz target 삭제, build 단계 생략, 실패 무시.
- 수용 기준: 깨끗한 환경에서 고정 도구 체인으로 `cargo check --manifest-path fuzz/Cargo.toml --locked`, `cargo fuzz list`, 두 target build가 base 대비 개선되고 두 번 연속 같은 결과를 냄.
- 사람 승인: CI 및 toolchain 고정 방식과 lockfile 변경을 구현 전에 승인.

### P0-3. base/head 차등 verifier 최소 구현

- 종류: `priority:p0`, `kind:infrastructure`
- 문제: 현재 PR 체크는 오래된 base 실행과 최신 head 실행을 단순 비교하므로 기존 실패와 신규 회귀를 신뢰성 있게 구분하지 못한다.
- 범위: 이 문서의 `baseline-v1` profile, canonical JSON, 종료 코드, fingerprint, base/head 격리 실행.
- 수용 기준: 합성 fixture로 여섯 판정 상태와 invalid input을 재현하고 같은 입력의 JSON digest가 일치함.
- 선행 조건: P0-1과 P0-2의 실패 fingerprint를 fixture로 확보.

### P0-4. Dependency Review 기반 기능 복구 결정

- 종류: `priority:p0`, `kind:infrastructure`, `kind:dependency`
- 문제: 워크플로는 존재하지만 Dependency graph 비활성으로 모든 PR에서 검사 불가다.
- 범위: 사람이 Dependency graph 활성화 여부를 결정한다. 활성화하면 PR에서 Dependency Review 성공을 확인하고, 활성화하지 않으면 해당 체크를 required gate에서 제외하며 `infrastructure_unavailable` 정책을 문서화한다.
- 금지: Hermes의 저장소 설정 직접 변경, 실패를 dependency 안전 통과로 해석.
- 수용 기준: 선택한 정책과 승인자가 기록되고, PR의 차등 판정에서 인프라 실패와 코드 실패가 분리됨.

## 18. 첫 두 파일럿 작업

### 파일럿 1: Clippy 기준선 복구

P0-1을 첫 작업으로 선택한다. 소스 변경 가능성이 있지만 범위가 diagnostic으로 제한되고, verifier의 `failed → passed` 판정을 가장 단순하게 검증할 수 있다.

진행 순서:

1. 사람이 이슈 본문과 허용 경로를 승인한다.
2. 구현 워커 A가 base 실패를 재현하고 최소 수정과 테스트를 커밋한다.
3. verifier가 base/head를 같은 stable toolchain에서 비교한다.
4. 리뷰 워커 B가 구현 워커와 분리된 세션에서 읽기 전용 리뷰를 한다.
5. 지적이 있으면 워커 A 또는 새 구현 워커가 repair하고 verification과 review를 반복한다.
6. 사람이 최종 병합 여부를 결정한다.

파일럿 성공 조건은 신규 실패 없이 Clippy가 개선되고, 역할 위반·잠금 충돌·수동 판정 덮어쓰기가 없는 것이다.

### 파일럿 2: fuzz 기준선 복구

P0-2를 두 번째 작업으로 선택한다. 설치 단계와 build 단계를 분리해 같은 체크 이름 아래 다른 원인을 fingerprint하는 능력을 검증한다.

진행 순서:

1. 사람이 nightly와 cargo-fuzz 고정 정책 및 허용 CI/fuzz 경로를 승인한다.
2. 구현 워커가 고정된 도구 체인으로 base 실패를 재현하고 최소 변경을 커밋한다.
3. verifier가 설치, workspace check, target list, target build를 독립 command ID로 비교한다.
4. 별도 리뷰 워커가 재현성, lockfile 영향, 실패 은폐 여부를 검토한다.
5. 두 번의 깨끗한 실행이 같은 verdict와 fingerprint를 만들 때 사람 병합 대기로 전이한다.

파일럿 2까지 완료하기 전에는 Dependabot PR을 자동 구현·병합 대상으로 넣지 않는다.

## 19. 성공 지표

### 기준선 지표

- 파일럿 종료 시 `main`의 Clippy와 Fuzz Target Build가 고정된 profile에서 통과한다.
- 기존 실패를 신규 회귀로 잘못 분류한 건수가 0건이다.
- 서로 다른 fuzz 실패 단계를 같은 fingerprint로 합친 건수가 0건이다.

### 운영 지표

- 모든 구현 handoff의 모델이 `openai/gpt-5.6-sol`이다.
- 모든 리뷰가 구현자와 다른 worker ID 및 세션에서 수행된다.
- Hermes의 코드 작성 또는 코드 리뷰 건수가 0건이다.
- 동시 OpenCode 워커가 2개를 넘은 시간이 0초다.
- 이중 잠금과 허용 경로 밖 변경이 0건이다.
- 모든 상태 전이와 사람 승인이 event hash chain으로 재구성된다.

### 품질 지표

- verifier 동일 입력 반복 실행의 canonical JSON digest 일치율이 100%다.
- `candidate_pass`로 분류된 파일럿에서 사람 또는 리뷰 워커가 발견한 기계적 신규 실패가 0건이다.
- repair는 이슈당 2회 이내이며, 초과 시 자동 중단 정책 준수율이 100%다.
- 두 파일럿 모두 리뷰 승인과 사람 승인을 거쳐야 완료된다.

## 20. 단계별 롤아웃

### 단계 0. 관찰 및 수동 준비

- 열린 이슈 0개 상태에서 P0-1부터 P0-4까지 후보 이슈 본문을 사람이 검토한다.
- 자동 GitHub 변경 없이 라벨 체계, 이슈 계약, verifier profile을 dry-run한다.
- 현재 `main`과 PR `#8`~`#13`의 SHA와 체크 결과를 snapshot으로 보존한다.
- 진입 기준: 두 파일럿의 범위와 승인자가 정해짐.

### 단계 1. 파일럿 1 단일 이슈

- 워커 동시성은 실질적으로 구현 1, 리뷰 1로 제한한다.
- Clippy 기준선 복구만 수행한다.
- 모든 상태 전이를 사람이 관찰한다.
- 종료 기준: 파일럿 1 성공 지표 충족 또는 중단 보고서 승인.

### 단계 2. 파일럿 2와 verifier 강화

- fuzz 도구 체인 고정과 build 복구를 수행한다.
- 단계별 fingerprint와 반복 실행 결정성을 검증한다.
- 종료 기준: 두 번 연속 동일 verdict, 별도 리뷰 승인, 사람 병합 결정.

### 단계 3. Dependency Review 정책 확정

- 사람이 Dependency graph 활성화 여부를 결정한다.
- 결과를 verifier의 인프라 상태와 GitHub required check 정책에 반영한다.
- 종료 기준: 모든 PR에서 Dependency Review 상태가 코드 회귀와 혼동되지 않음.

### 단계 4. 제한된 Dependabot 평가

- 먼저 base와 유효한 신규 실패가 없는 것으로 관찰된 `#11` 또는 `#12` 중 하나만 선택한다.
- 최신 `main`으로 갱신한 뒤 verifier와 분리 리뷰를 수행한다.
- 자동 병합은 계속 금지한다.
- 확대 기준: 오분류 0건, 역할 위반 0건, 사람 승인 후 완료 1건.

### 단계 5. 최대 2 워커 운영

- 경로가 독립적인 이슈에 한해 구현 2개 또는 구현 1개와 리뷰 1개를 운용한다.
- 10개 이슈 동안 성공 지표를 유지한 뒤에만 장기 서비스나 자동 병합을 별도 설계로 검토한다.

각 단계는 자동 승격하지 않는다. 사람 유지관리자가 이전 단계 지표와 감사 로그를 승인해야 다음 단계로 간다.

## 21. 대안과 선택 근거

### 대안 A. 기준선 복구 우선 이슈 루프와 동시 base/head verifier

이 문서가 선택한 방식이다.

장점:

- 현재 `main`의 실제 실패를 숨기지 않고 먼저 제거한다.
- 기존 실패와 PR 회귀를 같은 시점의 실행으로 구분한다.
- Hermes, 구현, 리뷰, 사람 승인 경계가 명확하다.
- 두 파일럿으로 작은 비용에 잠금, repair, 감사, 결정성을 검증할 수 있다.

단점:

- Dependabot 처리 속도가 즉시 빨라지지 않는다.
- base와 head를 모두 실행하므로 CI 비용이 늘어난다.

선택 근거: 현재는 열린 이슈가 없고 기준선 자체가 깨져 있어, 자동화 속도보다 판정 신뢰성과 복구 가능성이 우선이다.

### 대안 B. 기존 GitHub Actions 결과만 읽는 PR 우선 봇

Hermes가 PR `#8`~`#13`의 기존 체크를 읽고 성공한 PR부터 처리하는 방식이다.

장점:

- 구현 비용과 추가 CI 비용이 가장 작다.
- 기존 GitHub UI와 결과를 그대로 사용한다.

단점:

- 서로 다른 시점과 base SHA의 결과를 비교해 기존 Clippy/fuzz 실패를 신규 회귀로 오판할 수 있다.
- 같은 `Fuzz Target Build` 이름 아래 설치 실패와 workspace 실패를 구분하지 못한다.
- Dependency Review 인프라 실패가 모든 PR을 막거나 반대로 무시될 위험이 있다.

배제 근거: 현재 저장된 체크만으로는 안전한 base/head 차등 판정이 불가능하다.

### 대안 C. GitHub Actions 중심 완전 자율 멀티에이전트 플랫폼

웹훅, 큐, 데이터베이스, 자동 브랜치 생성, 자동 리뷰, 자동 병합을 한 번에 구축하는 방식이다.

장점:

- 장기적으로 높은 처리량과 중앙 관찰성을 제공할 수 있다.
- 여러 저장소로 확장하기 쉽다.

단점:

- 현재 두 개의 기준선 실패보다 훨씬 큰 운영·보안 범위를 만든다.
- 브랜치 보호와 Dependency graph가 정리되지 않은 상태에서 자동 병합 위험이 크다.
- 역할 분리와 deterministic verifier가 검증되기 전에 복잡한 상태와 권한을 도입한다.

배제 근거: MVP의 목표는 플랫폼 구축이 아니라 기준선 복구와 안전한 루프의 증명이다. 10개 이슈 운영 지표가 확보된 뒤 별도 설계 대상으로 검토한다.

## 22. 위험과 완화

| 위험 | 완화 |
|---|---|
| 실행 시점에 Rust나 cargo 도구가 바뀜 | exact version과 이미지 digest를 profile에 고정 |
| 과거 base 결과와 현재 head 결과를 비교 | 한 verifier run에서 base/head 재실행 |
| 동일 체크 이름의 다른 원인 | 단계별 command ID와 diagnostic fingerprint 사용 |
| 구현자 자기 승인 | 다른 worker ID·세션의 리뷰 handoff만 수락 |
| Hermes의 역할 침범 | 파일 쓰기·리뷰 생성 권한 제거, 이벤트 감사 |
| 워커 충돌 | 이슈별 원자 잠금, 경로 충돌 스케줄링, 최대 2 워커 |
| stale worktree 손실 | 자동 삭제 금지, lease 회수 전 dirty 상태와 SHA 보존 |
| Dependency Review 오판 | 코드 판정에서 인프라 상태 분리, 설정 변경은 사람 승인 |
| flaky 결과를 성공으로 둔갑 | 재실행은 비결정성 확인에만 사용하고 성공 승격 금지 |
| repair 무한 반복 | 2회 한도와 사람 재설계 지점 |

## 23. MVP 완료 조건

다음 조건을 모두 만족해야 기준선 복구 우선 MVP가 완료된다.

1. Clippy와 fuzz P0 이슈가 각각 구현 워커, verifier, 별도 리뷰 워커, 사람 승인을 거쳤다.
2. 고정 profile에서 `main`의 목표 실패가 통과로 개선되었다.
3. verifier가 base/head 차이를 canonical JSON과 정의된 종료 코드로 출력한다.
4. Dependency Review의 비활성 기반 기능을 코드 실패와 분리하는 정책이 사람에게 승인되었다.
5. 병렬성, 잠금, 허용 경로, 역할 분리 위반이 없다.
6. 모든 상태 전이와 승인 기록을 감사 로그에서 재구성할 수 있다.
7. Dependabot PR 자동 병합은 활성화되지 않았다.

이 완료 조건 이후에도 PR 병합과 GitHub 설정 변경은 사람 승인 사항으로 유지된다.
