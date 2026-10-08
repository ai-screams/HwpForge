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
pub(crate) fn build_picture(
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
    // A negative original size is refused, as for shapes
    // (`shapes::shape_frame_plan`).
    // 이것을 실패시키는 것: 이 거부를 지우기(`negative_original_size_is_not_written`).
    if let Some(o) = img.frame.original_size {
        if o.width.as_i32() < 0 || o.height.as_i32() < 0 {
            return Err(crate::error::HwpxError::InvalidStructure {
                detail: format!(
                    "picture has a negative original size {}x{}",
                    o.width.as_i32(),
                    o.height.as_i32()
                ),
            });
        }
    }
    let href = picture_href(img, sink);
    let half_w = w / 2;
    let half_h = h / 2;
    let placement = img.placement.as_ref();
    let frame = PictureFrame::of(img);

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
        href,
        group_level: 0,
        instid: generate_instid(),
        reverse: 0,

        offset: Some(HxOffset { x: 0, y: 0 }),
        org_sz: Some(HxSizeAttr { width: frame.original.0, height: frame.original.1 }),
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
            sca_matrix: crate::encoder::shapes::scale_matrix(frame.scale),
            rot_matrix: HxMatrix::identity(),
            extra_pairs: Vec::new(),
        }),
        img_rect: Some(HxImgRect {
            pt0: HxPoint { x: 0, y: 0 },
            pt1: HxPoint { x: frame.original.0, y: 0 },
            pt2: HxPoint { x: frame.original.0, y: frame.original.1 },
            pt3: HxPoint { x: 0, y: frame.original.1 },
        }),
        img_clip: Some(frame.clip),
        in_margin: Some(HxTableMargin { left: 0, right: 0, top: 0, bottom: 0 }),
        img_dim: frame.extent.map(|(dim_width, dim_height)| HxImgDim { dim_width, dim_height }),
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
            // 이것을 실패시키는 것: 상수 0 으로 되돌리기(`picture_and_table_protect_round_trip`).
            protect: u32::from(img.frame.size_protect),
        }),
        pos: Some(build_picture_position(placement)),
        out_margin: Some(crate::encoder::shapes::object_out_margin(placement)),
        caption: img
            .caption
            .as_ref()
            .map(|c| build_hx_caption(c, w, depth, hyperlink_entries, options, sink))
            .transpose()?,
        line_shape: None,
        effects: None,
    })
}

/// A picture's box and crop on the wire: `orgSz`/`imgRect` (the original
/// box O), `imgClip` (the crop in the image's coordinates I) and `imgDim`
/// (the extent of I). With none of the Core fields set every value is the
/// display size — the whole image, unscaled.
struct PictureFrame {
    /// `orgSz` (and the `imgRect` corners): the emission plan's original box
    /// ([`picture_frame_plan`](hwpforge_core::control::geometry::picture_frame_plan)).
    original: (i32, i32),
    /// `scaMatrix`: the plan's display / original.
    scale: (f64, f64),
    /// `imgClip`.
    clip: HxImgClip,
    /// `imgDim`; `None` = not written (a crop whose extent is unknown —
    /// Hancom's own files carry such pictures without `imgDim`).
    extent: Option<(i32, i32)>,
}

impl PictureFrame {
    fn of(img: &Image) -> Self {
        let display = (img.width.as_i32(), img.height.as_i32());
        let pair = |(a, b): (HwpUnit, HwpUnit)| (a.as_i32(), b.as_i32());
        let plan = hwpforge_core::control::geometry::picture_frame_plan(
            img,
            &crate::encoder::shapes::HWPX_POLICY,
        );
        let extent = img.image_extent.map(pair);
        let clip = match img.crop {
            Some(c) => HxImgClip {
                left: c.left.as_i32(),
                right: c.right.as_i32(),
                top: c.top.as_i32(),
                bottom: c.bottom.as_i32(),
            },
            None => {
                let (ew, eh) = extent.unwrap_or(display);
                HxImgClip { left: 0, right: ew, top: 0, bottom: eh }
            }
        };
        let extent = match (img.crop, extent) {
            (Some(_), None) => None,
            (_, extent) => Some(extent.unwrap_or(display)),
        };
        Self { original: plan.original_size(), scale: plan.scale(), clip, extent }
    }
}

/// The picture's `href`: its link URL with Hancom's object-link suffix, or
/// empty. A URL with an unsafe scheme (`javascript:`, `file:`, …) is not
/// written and is reported as lost; a schemeless one gets `http://`, as a
/// text hyperlink does.
fn picture_href(img: &Image, sink: &mut EncodeSink) -> String {
    let Some(url) = img.hyperlink.as_deref() else { return String::new() };
    match super::super::normalize_hyperlink_url(url) {
        Some(safe) if !safe.is_empty() => {
            format!("{safe}{}", crate::decoder::section::OBJECT_HYPERLINK_SUFFIX)
        }
        _ => {
            sink.content_lost(
                "picture_hyperlink",
                format!("picture hyperlink {url:?} is not a safe URL and is left out"),
            );
            String::new()
        }
    }
}

pub(super) fn build_picture_position(placement: Option<&ObjectPlacement>) -> HxTablePos {
    match placement {
        Some(value) => HxTablePos {
            treat_as_char: u32::from(value.treat_as_char),
            affect_l_spacing: u32::from(value.affect_line_spacing),
            flow_with_text: u32::from(value.flow_with_text),
            allow_overlap: u32::from(value.allow_overlap),
            hold_anchor_and_so: u32::from(value.hold_anchor_and_so),
            vert_rel_to: value.vert_rel_to.as_hwpx_str().into_owned(),
            horz_rel_to: value.horz_rel_to.as_hwpx_str().into_owned(),
            vert_align: value.vert_align.as_hwpx_str().to_string(),
            horz_align: value.horz_align.as_hwpx_str().to_string(),
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
