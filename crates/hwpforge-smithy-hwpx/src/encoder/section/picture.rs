//! Inline picture (`<hp:pic>`) builders (task #92 split from
//! `encoder/section.rs`). Floating-shape pictures live in
//! `encoder/shapes.rs`; this module builds the paragraph-inline
//! variant.

use super::*;

/// Builds `HxTable` from a Core `Table`.
///
/// Populates all attributes and sub-elements required by 한글:
/// `hp:sz`, `hp:pos`, `hp:outMargin`, `hp:inMargin`, plus full
/// attribute set on `<hp:tbl>`.
///
/// Builds `HxPic` from a Core `Image` with complete shape structure.
///
/// The `BinData/` prefix and file extension are stripped from the path
/// to produce the `binaryItemIDRef` attribute value. For example,
/// `"BinData/image1.png"` becomes `"image1"`. This matches 한글's
/// convention where `binaryItemIDRef` is a logical name without extension.
///
/// Generates all required sub-elements (offset, orgSz, curSz, flip,
/// rotationInfo, renderingInfo, imgRect, imgClip, inMargin, imgDim,
/// img, sz, pos, outMargin) to match 한글's expected structure.
pub(super) fn build_picture(
    img: &Image,
    depth: usize,
    hyperlink_entries: &mut Vec<(String, String)>,
    options: EncodeOptions,
    sink: &mut EncodeSink,
) -> HwpxResult<HxPic> {
    let without_prefix = img.path.strip_prefix("BinData/").unwrap_or(&img.path);
    // Strip extension: "image1.png" → "image1"
    let binary_ref = match without_prefix.rfind('.') {
        Some(dot) => &without_prefix[..dot],
        None => without_prefix,
    };

    let w = img.width.as_i32();
    let h = img.height.as_i32();
    let half_w = w / 2;
    let half_h = h / 2;
    let placement = img.placement.as_ref();

    Ok(HxPic {
        // Wave 12p Step 4: HWPX `<hp:pic id="...">` cross-ref target.
        // Image.inst_id 가 있으면 사용 (한컴 native 의 instance ID),
        // 없으면 sequential fallback.
        id: img.inst_id.map(|n| n.to_string()).unwrap_or_else(generate_instid),
        z_order: 0,
        numbering_type: "PICTURE".to_string(),
        text_wrap: placement
            .map(|value| value.text_wrap.as_hwpx_str().into_owned())
            .unwrap_or_else(|| "TOP_AND_BOTTOM".to_string()),
        text_flow: placement
            .map(|value| value.text_flow.as_hwpx_str().into_owned())
            .unwrap_or_else(|| "BOTH_SIDES".to_string()),
        lock: 0,
        dropcap_style: DropCapStyle::None.to_string(),
        href: String::new(),
        group_level: 0,
        instid: generate_instid(),
        reverse: 0,

        offset: Some(HxOffset { x: 0, y: 0 }),
        org_sz: Some(HxSizeAttr { width: w, height: h }),
        cur_sz: Some(HxSizeAttr { width: w, height: h }),
        flip: Some(HxFlip { horizontal: 0, vertical: 0 }),
        rotation_info: Some(HxRotationInfo {
            angle: 0,
            center_x: half_w,
            center_y: half_h,
            rotate_image: 1,
        }),
        rendering_info: Some(HxRenderingInfo {
            trans_matrix: HxMatrix::identity(),
            sca_matrix: HxMatrix::identity(),
            rot_matrix: HxMatrix::identity(),
        }),
        img_rect: Some(HxImgRect {
            pt0: HxPoint { x: 0, y: 0 },
            pt1: HxPoint { x: w, y: 0 },
            pt2: HxPoint { x: w, y: h },
            pt3: HxPoint { x: 0, y: h },
        }),
        img_clip: Some(HxImgClip { left: 0, right: w, top: 0, bottom: h }),
        in_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        img_dim: Some(HxImgDim { dim_width: w, dim_height: h }),
        img: Some(HxImg {
            binary_item_id_ref: binary_ref.to_string(),
            bright: 0,
            contrast: 0,
            effect: "REAL_PIC".to_string(),
            alpha: "0".to_string(),
        }),
        sz: Some(HxTableSz {
            width: w,
            width_rel_to: "ABSOLUTE".to_string(),
            height: h,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(build_picture_position(placement)),
        out_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        caption: img
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
            .transpose()?,
    })
}

pub(super) fn build_picture_position(placement: Option<&ObjectPlacement>) -> HxTablePos {
    match placement {
        Some(value) => HxTablePos {
            treat_as_char: u32::from(value.treat_as_char),
            affect_l_spacing: 0,
            flow_with_text: u32::from(value.flow_with_text),
            allow_overlap: u32::from(value.allow_overlap),
            hold_anchor_and_so: 0,
            vert_rel_to: value.vert_rel_to.as_hwpx_str().into_owned(),
            horz_rel_to: value.horz_rel_to.as_hwpx_str().into_owned(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: value.vert_offset.as_i32(),
            horz_offset: value.horz_offset.as_i32(),
        },
        None => HxTablePos {
            treat_as_char: 1,
            affect_l_spacing: 0,
            flow_with_text: 0,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "PARA".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        },
    }
}
