use super::*;
use std::collections::HashSet;

// 이것을 실패시키는 것: 같은 (소유자, namespace, 이름)을 두 줄에 적어 앞 줄의 판정이 뒤 줄을 가리는 것.
#[test]
fn table_vocabulary_names_each_owner_and_name_once() {
    let mut seen = HashSet::new();
    for (owners, names, namespace, _, _) in TABLE_VOCABULARY {
        for owner in owners.split_ascii_whitespace() {
            for name in names.split_ascii_whitespace() {
                assert!(
                    seen.insert((owner, *namespace, name)),
                    "duplicate vocabulary entry: {owner} {name}"
                );
            }
        }
    }
}
