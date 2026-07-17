# Loop Engineering 운영 정책

## 활성 정책

- 활성 정책: `activate`
- 승인자: Han-taz
- 승인 시각(UTC): 2026-07-17T14:27:00Z
- 대상 SHA: 4cd693d1b801c99a6ebf09ce168a33d379725038

이 결정은 사람의 승인으로 확정되었으며 자동으로 변경하지 않는다.

## Dependency Review 인프라 상태

Hermes가 독립적으로 확인한 불변 증거는 다음과 같다.

- PUT /repos/Han-taz/hwpx-rust/vulnerability-alerts: HTTP 204
- GET /repos/Han-taz/hwpx-rust/vulnerability-alerts: HTTP 204
- dependency-graph SBOM: SPDXRef-DOCUMENT

이 상태는 Dependency Review 인프라의 활성 여부만 나타내며 코드 델타 판정과 분리한다. infrastructure_unavailable is not a dependency safety pass.

## 코드 델타 판정

Dependency Review는 `actions/dependency-review-action@v5.0.0`, `fail-on-severity: moderate`, `fail-on-scopes: runtime, unknown` 계약으로 코드 델타를 판정한다. 이 판정은 필수 게이트로서 통과해야 하지만 자동 병합하지 않는다.

## 롤백과 권한

롤백은 사람의 승인을 받은 뒤 공식 vulnerability-alerts 비활성화 경로 `DELETE /repos/{owner}/{repo}/vulnerability-alerts`를 사용한다. 자동 롤백은 하지 않는다.

Hermes는 증거 확인과 보고만 담당한다. Hermes는 승인, 정책 변경, 롤백 또는 병합을 수행하지 않는다.
