//! HWPX input slots reject CFB only after an input decode failure.
#![cfg(feature = "ops-md")]

use hwpforge::hwpx::{stamp::StampMap, HwpxDecoder};
use hwpforge::ops::*;
use quote::ToTokens;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};
use syn::visit::Visit;

#[path = "support/fixtures.rs"]
mod fixtures;

const CFB: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const HWP5: &[u8] = include_bytes!("data/hwp5-dangling-table-border-fill.hwp");

struct Context {
    good: Vec<u8>,
    document: String,
    section: String,
}
impl Context {
    fn new() -> Self {
        let good = fixtures::hwpx_fixture("SimpleTable.hwpx");
        let document =
            serde_json::to_string(&to_json(&good, &ToJsonOptions::default()).unwrap().document)
                .unwrap();
        let section = serde_json::to_string(
            &export_section(&good, &ExportSectionOptions::default()).unwrap().section,
        )
        .unwrap();
        Self { good, document, section }
    }
}

struct Input {
    function: &'static str,
    slot: &'static str,
    reject: fn(&[u8], &Context) -> Option<OpsError>,
}

// The registry drives both the syntax audit and actual input calls.
const INPUTS: &[Input] = &[
    Input {
        function: "inspect",
        slot: "hwpx",
        reject: |b, _| inspect(b, &InspectOptions::default()).err(),
    },
    Input { function: "outline", slot: "hwpx", reject: |b, _| outline(b).err() },
    Input { function: "fields", slot: "hwpx", reject: |b, _| fields(b).err() },
    Input {
        function: "read",
        slot: "hwpx",
        reject: |b, _| read(b, &ReadOptions::default().with_section(0)).err(),
    },
    Input {
        function: "to_md",
        slot: "hwpx",
        reject: |b, _| to_md(b, &MdExportOptions::default()).err(),
    },
    Input {
        function: "to_json",
        slot: "hwpx",
        reject: |b, _| to_json(b, &ToJsonOptions::default()).err(),
    },
    Input {
        function: "export_section",
        slot: "hwpx",
        reject: |b, _| export_section(b, &ExportSectionOptions::default()).err(),
    },
    Input {
        function: "from_json",
        slot: "opts.base",
        reject: |b, c| from_json(&c.document, &FromJsonOptions::default().with_base(b)).err(),
    },
    Input {
        function: "patch",
        slot: "hwpx",
        reject: |b, c| patch(b, &PatchOptions::default().with_patch(&c.section)).err(),
    },
    Input {
        function: "restyle",
        slot: "hwpx",
        reject: |b, _| restyle(b, &RestyleOptions::default().with_preset("modern")).err(),
    },
    Input { function: "validate", slot: "hwpx", reject: |b, _| validate(b).err() },
    Input { function: "diff", slot: "base", reject: |b, c| diff(b, &c.good).err() },
    Input { function: "diff", slot: "revised", reject: |b, c| diff(&c.good, b).err() },
    Input {
        function: "fill",
        slot: "hwpx",
        reject: |b, _| fill(b, &[("name".into(), "value".into())], &FillOptions::default()).err(),
    },
    Input {
        function: "set_cell",
        slot: "hwpx",
        reject: |b, _| {
            set_cell(b, &SetCellOptions::default().with_table(0).with_at("0,0").with_text("value"))
                .err()
        },
    },
    Input {
        function: "insert_para",
        slot: "hwpx",
        reject: |b, _| {
            insert_para(b, &InsertParaOptions::default().with_text(vec!["text".into()])).err()
        },
    },
    Input {
        function: "delete_para",
        slot: "hwpx",
        reject: |b, _| delete_para(b, &DeleteParaOptions::default().with_indexes(vec![1])).err(),
    },
    Input { function: "stamp_plan", slot: "hwpx", reject: |b, _| stamp_plan(b).err() },
    Input {
        function: "stamp",
        slot: "hwpx",
        reject: |b, _| stamp(b, &StampMap::Legacy(vec![]), &StampOptions::default()).err(),
    },
];

#[test]
// 이것을 실패시키는 것: 각 입력 오류 경계의 재분류 호출 삭제 또는 HWP5_INPUT 코드 변경.
fn every_registered_input_rejects_hwp5_and_minimal_cfb() {
    let context = Context::new();
    for input in INPUTS {
        for bytes in [CFB, HWP5] {
            let error = (input.reject)(bytes, &context).unwrap();
            assert_eq!(
                error.code().as_str(),
                "HWP5_INPUT",
                "{}.{}: {error}",
                input.function,
                input.slot
            );
            assert!(error.to_string().contains("ZIP"), "original cause: {error}");
        }
    }
}

#[test]
// 이것을 실패시키는 것: CFB 접두사 조건을 항상 참으로 바꾸기 또는 8바이트 대신 일부만 검사.
fn short_and_pk_inputs_keep_decode_failed() {
    for bytes in [b"PK".as_slice(), b"broken", &CFB[..7], b""] {
        assert_eq!(
            inspect(bytes, &InspectOptions::default()).unwrap_err().code().as_str(),
            "DECODE_FAILED"
        );
    }
}

#[test]
// 이것을 실패시키는 것: 등록된 입력 중 하나를 CFB 접두사만으로 미리 거부.
fn valid_prefixed_packages_still_decode() {
    let context = Context::new();
    let good = &context.good;
    let expected = serde_json::to_value(fields(good).unwrap().meta()).unwrap();
    for prefix in [b"arbitrary prefix".as_slice(), CFB] {
        let bytes = [prefix, good.as_slice()].concat();
        for input in INPUTS {
            if let Some(error) = (input.reject)(&bytes, &context) {
                assert_ne!(
                    error.code().as_str(),
                    "HWP5_INPUT",
                    "{}.{}: {error}",
                    input.function,
                    input.slot
                );
            }
        }
        assert_eq!(serde_json::to_value(fields(&bytes).unwrap().meta()).unwrap(), expected);
        assert!(diff(&bytes, &bytes).unwrap().diff.identical);
    }
}

fn broken_xml_package() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("mimetype", "application/hwp+zip"), ("Contents/header.xml", "<broken")] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
// 이것을 실패시키는 것: ZIP 열림 조건 삭제, base 재디코드를 ZIP probe로 대체, revised만 검사, base만 검사.
fn diff_preserves_the_first_failed_input_and_xml_cause() {
    let good = Context::new().good;
    let broken = broken_xml_package();
    let cause = HwpxDecoder::decode(&broken).unwrap_err().to_string();
    assert!(cause.contains("XML parse error"), "{cause}");
    for prefix in [b"".as_slice(), CFB] {
        let valid_base = [prefix, &good].concat();
        assert_eq!(diff(&valid_base, HWP5).unwrap_err().code().as_str(), "HWP5_INPUT");
        let broken_base = [prefix, &broken].concat();
        let error = diff(&broken_base, HWP5).unwrap_err();
        assert_eq!(error.code().as_str(), "DECODE_FAILED");
        assert_eq!(error.to_string(), cause);
    }
    assert_eq!(diff(b"broken", HWP5).unwrap_err().code().as_str(), "DECODE_FAILED");
    assert_eq!(diff(HWP5, b"broken").unwrap_err().code().as_str(), "HWP5_INPUT");
    assert_eq!(diff(&[CFB, b"broken"].concat(), HWP5).unwrap_err().code().as_str(), "HWP5_INPUT");
    assert_eq!(diff(HWP5, HWP5).unwrap_err().code().as_str(), "HWP5_INPUT");
    assert_eq!(
        inspect(&[CFB, &broken].concat(), &InspectOptions::default()).unwrap_err().code().as_str(),
        "DECODE_FAILED"
    );
}

#[test]
// 이것을 실패시키는 것: fill·set_cell·from_json 인자 검사보다 CFB 재분류를 먼저 실행.
fn argument_errors_still_precede_decoding() {
    assert_eq!(fill(CFB, &[], &FillOptions::default()).unwrap_err().code().as_str(), "NO_VALUES");
    assert_eq!(
        set_cell(CFB, &SetCellOptions::default()).unwrap_err().code().as_str(),
        "INVALID_SET_CELL_ARGS"
    );
    assert_eq!(
        from_json("{", &FromJsonOptions::default().with_base(CFB)).unwrap_err().code().as_str(),
        "JSON_PARSE_FAILED"
    );
}

#[test]
// 이것을 실패시키는 것: read의 table 또는 field 분기에서 재분류 호출 삭제.
fn read_targets_classify_their_input() {
    for options in [ReadOptions::default().with_table(0), ReadOptions::default().with_field("name")]
    {
        assert_eq!(read(HWP5, &options).unwrap_err().code().as_str(), "HWP5_INPUT");
    }
}

// Inspect public free functions and methods, following option structs.
// Unsupported byte containers fail closed instead of disappearing from the audit.
#[derive(Default)]
struct Inventory {
    functions: Vec<(String, syn::Signature)>,
    structs: BTreeMap<String, syn::ItemStruct>,
    owner: String,
}
impl<'ast> Visit<'ast> for Inventory {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if matches!(node.vis, syn::Visibility::Public(_)) {
            self.functions.push((node.sig.ident.to_string(), node.sig.clone()));
        }
    }
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let previous =
            std::mem::replace(&mut self.owner, node.self_ty.to_token_stream().to_string());
        syn::visit::visit_item_impl(self, node);
        self.owner = previous;
    }
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if matches!(node.vis, syn::Visibility::Public(_)) {
            self.functions.push((format!("{}::{}", self.owner, node.sig.ident), node.sig.clone()));
        }
    }
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.structs.insert(node.ident.to_string(), node.clone());
    }
}

fn contains_byte_type(ty: &syn::Type) -> bool {
    struct Bytes(bool);
    impl<'ast> Visit<'ast> for Bytes {
        fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
            self.0 |= node.path.is_ident("u8");
            syn::visit::visit_type_path(self, node);
        }
    }
    let mut bytes = Bytes(false);
    bytes.visit_type(ty);
    bytes.0
}

fn byte_slots(
    ty: &syn::Type,
    path: String,
    structs: &BTreeMap<String, syn::ItemStruct>,
) -> Vec<String> {
    if let syn::Type::Reference(reference) = ty {
        return byte_slots(&reference.elem, path, structs);
    }
    let text = ty.to_token_stream().to_string().replace(' ', "");
    if ["[u8]", "Vec<u8>", "Option<Vec<u8>>"].contains(&text.as_str()) {
        return vec![path];
    }
    assert!(!contains_byte_type(ty), "unsupported byte input {path}: {text}");
    let name = match ty {
        syn::Type::Path(ty) => ty.path.segments.last().unwrap().ident.to_string(),
        _ => return vec![],
    };
    structs
        .get(&name)
        .into_iter()
        .flat_map(|item| item.fields.iter())
        .flat_map(|field| {
            byte_slots(&field.ty, format!("{path}.{}", field.ident.as_ref().unwrap()), structs)
        })
        .collect()
}

fn inventory_slots(inventory: &Inventory) -> BTreeSet<(String, String)> {
    let mut actual = BTreeSet::new();
    for (name, signature) in &inventory.functions {
        for arg in &signature.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                // This builder only stores bytes. from_json's opts.base entry
                // covers decoding them; all other byte methods are audited.
                if name == "FromJsonOptions::with_base" {
                    assert_eq!(
                        arg.ty.to_token_stream().to_string().replace(' ', ""),
                        "implInto<Vec<u8>>"
                    );
                    continue;
                }
                let path = arg.pat.to_token_stream().to_string();
                for slot in byte_slots(&arg.ty, path, &inventory.structs) {
                    actual.insert((name.clone(), slot));
                }
            }
        }
    }
    actual
}

#[test]
// 이것을 실패시키는 것: 미등록 공개 &[u8] 함수·옵션 Vec<u8> 필드 추가, opts.base 등록 삭제,
// 공개 함수·파일·바이트 타입·참조 타입·디렉터리·인자 순회 조건 제거 또는 중복 등록.
fn public_byte_input_inventory_matches_behavior_registry() {
    // Start above ops so even today's tree exercises recursive traversal;
    // a future directory module cannot escape the public-input audit.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut directories = vec![dir];
    let mut inventory = Inventory::default();
    while let Some(dir) = directories.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                inventory
                    .visit_file(&syn::parse_file(&std::fs::read_to_string(path).unwrap()).unwrap());
            }
        }
    }
    let actual = inventory_slots(&inventory);
    let expected: BTreeSet<_> =
        INPUTS.iter().map(|input| (input.function.to_owned(), input.slot.to_owned())).collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 19);
    assert_eq!(actual.iter().map(|(name, _)| name).collect::<BTreeSet<_>>().len(), 18);
    assert_eq!(expected.len(), INPUTS.len(), "duplicate registry entry");
}

#[test]
// 이것을 실패시키는 것: stamp v2 재분류 호출 삭제 또는 해시 검사보다 CFB 거부를 먼저 실행.
fn stamp_v2_classifies_only_after_hash_validation() {
    let request = hwpforge::hwpx::stamp::StampRequestV2 {
        schema_version: 2,
        source_sha256: "1d90eb2fd41a9148f4caa705a4551611db4ea97a7ab0afae204b65ebe193fd77".into(),
        text: vec![],
        cells: vec![],
    };
    assert_eq!(
        stamp(HWP5, &StampMap::V2(request.clone()), &StampOptions::default())
            .unwrap_err()
            .code()
            .as_str(),
        "HWP5_INPUT"
    );
    assert_eq!(
        stamp(CFB, &StampMap::V2(request), &StampOptions::default()).unwrap_err().code().as_str(),
        "STAMP_SOURCE_HASH_MISMATCH"
    );
}

#[test]
// 이것을 실패시키는 것: ZIP 열림 조건을 삭제하여 CFB 접두사의 패키지 비교 실패를 재분류.
fn diff_package_comparison_failure_is_not_an_input_classification() {
    let mut writer = zip::ZipWriter::new_append(Cursor::new(Context::new().good)).unwrap();
    let marker = b"only package comparison reads this payload";
    writer
        .start_file(
            "unreferenced.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    writer.write_all(marker).unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    let at = bytes.windows(marker.len()).position(|w| w == marker).unwrap();
    bytes[at] ^= 1; // Leave the directory valid but break this entry's CRC.
    let bytes = [CFB, &bytes].concat();
    HwpxDecoder::decode(&bytes).expect("initial decode must succeed");
    let error = diff(&bytes, &bytes).unwrap_err();
    assert_eq!(error.code().as_str(), "DECODE_FAILED");
    assert!(error.to_string().contains("Invalid checksum"), "{error}");
}

#[test]
// 이것을 실패시키는 것: 메서드 방문·마지막 경로 세그먼트 조회·수명 참조 해제 삭제.
fn inventory_follows_methods_qualified_options_and_lifetimes() {
    let mut inventory = Inventory::default();
    inventory.visit_file(&syn::parse_quote! {
        struct Options<'a> { bytes: &'a [u8] }
        struct Reader;
        impl Reader {
            pub fn read(&self, opts: &crate::Options<'_>, direct: &[u8]) {}
            fn private(&self, bytes: &[u8]) {}
        }
    });
    assert_eq!(
        inventory_slots(&inventory),
        BTreeSet::from([
            ("Reader::read".into(), "opts.bytes".into()),
            ("Reader::read".into(), "direct".into()),
        ])
    );
}

#[test]
// 이것을 실패시키는 것: 미지원 바이트 타입 거부를 제거해 빈 인벤토리로 통과시키기.
fn inventory_refuses_unsupported_byte_shapes() {
    for ty in ["impl AsRef<[u8]>", "Cow<'_, [u8]>", "&[&[u8]]", "[u8; 8]", "Option<&'a [u8]>"] {
        for declaration in [
            format!("pub fn read(bytes: {ty}) {{}}"),
            format!("struct Options<'a> {{ bytes: {ty} }} pub fn read(opts: &Options<'_>) {{}}"),
            format!("struct Reader; impl Reader {{ pub fn read(&self, bytes: {ty}) {{}} }}"),
        ] {
            let mut inventory = Inventory::default();
            inventory.visit_file(&syn::parse_file(&declaration).unwrap());
            assert!(
                std::panic::catch_unwind(|| inventory_slots(&inventory)).is_err(),
                "{declaration}"
            );
        }
    }
}

#[test]
// 이것을 실패시키는 것: HWP5 오류의 source 제거·원래 오류 대체·Display 접두사 제거.
fn hwp5_input_preserves_the_original_error_chain() {
    use std::error::Error;
    let original = HwpxDecoder::decode(CFB).unwrap_err();
    let error = fields(CFB).unwrap_err();
    let source = error.source().unwrap().downcast_ref::<Box<OpsError>>().unwrap().as_ref();
    assert!(matches!(source, OpsError::Decode(_)));
    assert_eq!(source.to_string(), original.to_string());
    assert_eq!(
        source.source().map(ToString::to_string),
        original.source().map(ToString::to_string)
    );
    assert_eq!(
        error.to_string(),
        format!("HWP5 (.hwp) input requires conversion to HWPX first; cause: {original}")
    );
    assert_eq!(
        error.hint(),
        Some("Convert the HWP5 (.hwp) document to HWPX before using this operation.")
    );
}

#[test]
// 이것을 실패시키는 것: ZIP 정책 거부까지 HWP5_INPUT으로 재분류.
fn prefixed_zip_policy_failures_keep_decode_failed() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in ["first", "other"] {
        writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(b"x").unwrap();
    }
    let mut duplicate = writer.finish().unwrap().into_inner();
    // Alter both physical names without asking the writer to accept duplicates.
    for index in 0..duplicate.len() - 4 {
        if &duplicate[index..index + 5] == b"other" {
            duplicate[index..index + 5].copy_from_slice(b"first");
        }
    }
    let mut excessive = duplicate.clone();
    let end = excessive.len() - 22;
    excessive[end + 8..end + 12].copy_from_slice(&[0x11, 0x27, 0x11, 0x27]);
    for (bytes, reason) in [(duplicate, "duplicate"), (excessive, "exceeds limit")] {
        let bytes = [CFB, &bytes].concat();
        let original = HwpxDecoder::decode(&bytes).unwrap_err().to_string();
        assert!(original.contains(reason), "{original}");
        let context = Context::new();
        for input in INPUTS {
            let error = (input.reject)(&bytes, &context).unwrap();
            assert_eq!(
                error.code().as_str(),
                "DECODE_FAILED",
                "{}.{}: {error}",
                input.function,
                input.slot
            );
            assert!(error.to_string().contains(reason), "{error}");
        }
    }
}

#[test]
// 이것을 실패시키는 것: facade·convert·CLI 중 CFB 리터럴 하나를 변경.
fn cfb_signatures_match_the_container_literal() {
    for source in [
        include_str!("../src/ops/input.rs"),
        include_str!("../../hwpforge-convert/src/ops/pdf.rs"),
        include_str!("../../hwpforge-bindings-cli/src/commands/to_pdf.rs"),
    ] {
        let file = syn::parse_file(source).unwrap();
        let item = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Const(item) if item.ident == "CFB_MAGIC" => Some(item),
                _ => None,
            })
            .unwrap();
        let syn::Expr::Array(array) = item.expr.as_ref() else {
            panic!("expected literal signature")
        };
        let bytes: Vec<u8> = array
            .elems
            .iter()
            .map(|expr| {
                let syn::Expr::Lit(expr) = expr else { panic!("expected byte literal") };
                let syn::Lit::Int(value) = &expr.lit else { panic!("expected integer") };
                value.base10_parse().unwrap()
            })
            .collect();
        assert_eq!(bytes, [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    }
}
