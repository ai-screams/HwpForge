# Benchmarks

이 디렉터리에는 현재 벤치마크 코드가 없습니다. README와 `AGENTS.md`만 있고 `.rs` 파일이 없으며, 어떤 `Cargo.toml`에도 `[[bench]]` 타깃이 없습니다.

- 루트 `Cargo.toml`의 `[workspace.dependencies]`에 `criterion` 버전만 선언돼 있고, 이를 가져다 쓰는 크레이트는 없습니다.
- `crates/hwpforge-foundation/Cargo.toml`에는 `# criterion deferred -- enable when benchmarks are needed` 주석이 있습니다. 벤치마크가 필요해질 때 켜기로 미뤄 둔 상태입니다.
- 따라서 `cargo bench`는 실행할 벤치마크가 없고 `target/criterion/` 리포트도 만들어지지 않습니다.

벤치마크를 추가한다면 대상 크레이트에 `criterion`을 dev-dependency로 넣고 `[[bench]]` 타깃을 선언한 뒤, 이 문서를 실제 구성에 맞게 고칩니다.
