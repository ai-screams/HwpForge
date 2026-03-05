//! HwpForge Foundation: primitive types for the HwpForge ecosystem.
//!
//! This crate sits at the bottom of the dependency graph. It provides
//! raw-material value types used by every other HwpForge crate:
//!
//! - [`HwpUnit`] -- the universal measurement unit (1/100 pt)
//! - [`Color`] -- BGR color matching the HWP specification
//! - [`FontId`], [`TemplateName`], [`StyleName`] -- string-based identifiers
//! - [`Index<T>`] -- branded numeric indices with phantom-type safety
//! - [`Alignment`], [`LineSpacingType`], [`BreakType`], [`Language`], [`WordBreakType`] -- core enums
//! - [`FoundationError`], [`ErrorCode`] -- structured error handling
//!
//! # Design Principles
//!
//! - **Zero-cost newtypes**: `repr(transparent)` on value wrappers
//! - **Type safety**: phantom-branded indices prevent mixing
//! - **Compile-time guarantees**: `const` size assertions
//! - **No unsafe code**: `#![deny(unsafe_code)]`

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod color;
pub mod enums;
pub mod error;
pub mod ids;
pub mod index;
mod macros;
pub mod units;

pub use color::Color;
pub use enums::{
    Alignment, ApplyPageType, ArcType, ArrowSize, ArrowType, BookmarkType, BorderLineType,
    BreakType, CurveSegmentType, DropCapStyle, EmbossType, EmphasisType, EngraveType, FieldType,
    FillBrushType, Flip, GradientType, GutterType, HeadingType, ImageFillMode, Language,
    LineSpacingType, NumberFormatType, OutlineType, PageNumberPosition, PatternType,
    RefContentType, RefType, RestartType, ShadowType, ShowMode, StrikeoutShape, TextBorderType,
    TextDirection, UnderlineType, VerticalPosition, WordBreakType,
};
pub use error::{ErrorCode, ErrorCodeExt, FoundationError, FoundationResult};
pub use ids::{FontId, StyleName, TemplateName};
pub use index::{
    BorderFillIndex, CharShapeIndex, FontIndex, Index, NumberingIndex, ParaShapeIndex, StyleIndex,
    TabIndex,
};
pub use units::{HwpUnit, Insets, Point, Rect, Size};
