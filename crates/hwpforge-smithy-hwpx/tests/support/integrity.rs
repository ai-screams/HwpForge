// Descriptor inputs shared by the common guard and editing regressions.

// 이것을 실패시키는 것: namespace 정규화 제거 또는 정규화 실패를 허용.
pub const REFUSED: &[&str] = &[
    "<s:Signature xmlns:s='http://www.w3.org/2000/09/xmldsig&#35;'/>",
    "<Signature xmlns='http://www.w3.org/2000/09/xmldsig&#x23;'/>",
    "<signatures xmlns='urn:oasis:names:tc:opendocument:xmlns:containe&#114;'/>",
    "<s:signatures xmlns:s='urn:oasis:names:tc:opendocument:xmlns:containe&#x72;'/>",
    "<encryption-data xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.&#48;'/>",
    "<e:encryption-data xmlns:e='urn:oasis:names:tc:opendocument:xmlns:manifest:1.&#x30;'/>",
    "<item xmlns='http://www.idpf.org/2007/op&#102;/' encryption='tr&#117;e'/>",
    "<o:item xmlns:o='http://www.idpf.org/2007/op&#x66;' encryption='&#x31;'/>",
    "<r xmlns='urn:broken&unknown;'/>",
    "<r xmlns='urn:broken&#xZZ;'/>",
    "<r xmlns='urn:broken&#0;'/>",
    "<r xmlns='urn:broken&'/>",
    // No protected URI contains one of the five predefined entity characters.
    // A predefined ampersand is valid, but must not hide a later invalid entity.
    "<r xmlns='urn:&amp;&unknown;'/>",
];

// 이것을 실패시키는 것: namespace 이중 정규화 또는 모든 entity namespace 거부.
pub const EDITABLE: &[&str] = &[
    "<item xmlns='http://www.idpf.org/2007/opf/' encryption='false'/>",
    "<item xmlns='http://www.idpf.org/2007/opf/' encryption='&#102;alse'/>",
    "<item xmlns='http://www.idpf.org/2007/opf/' encryption='&#x30;'/>",
    "<Signature xmlns='urn:&amp;&lt;&gt;&apos;&quot;'/>",
    "<Signature xmlns='http://www.w3.org/2000/09/xmldsig&amp;#35;'/>",
];
