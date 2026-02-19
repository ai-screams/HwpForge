//! Chart types for OOXML-based chart support.
//!
//! Charts in HWPX use the OOXML chart XML format (`xmlns:c`).
//! This module defines the chart type enum (18 variants covering all 16
//! OOXML chart types, with Bar/Column direction split) and the data model
//! for category-based and XY-based chart data.
//!
//! # Examples
//!
//! ```
//! use hwpforge_core::chart::{ChartType, ChartData, ChartGrouping, LegendPosition};
//!
//! let data = ChartData::category(
//!     &["Q1", "Q2", "Q3"],
//!     &[("Sales", &[100.0, 150.0, 200.0])],
//! );
//! assert!(matches!(data, ChartData::Category { .. }));
//! ```

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// OOXML chart types supported by 한글 (16 OOXML types → 18 variants).
///
/// Bar and Column are both `<c:barChart>` in OOXML, distinguished by
/// `<c:barDir val="bar|col">`. Similarly for 3D variants.
///
/// # Examples
///
/// ```
/// use hwpforge_core::chart::ChartType;
///
/// let ct = ChartType::Column;
/// assert_eq!(format!("{ct:?}"), "Column");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum ChartType {
    /// Horizontal bar chart (`<c:barChart>` with `barDir=bar`).
    Bar,
    /// Vertical bar chart (`<c:barChart>` with `barDir=col`).
    Column,
    /// 3D horizontal bar chart (`<c:bar3DChart>` with `barDir=bar`).
    Bar3D,
    /// 3D vertical bar chart (`<c:bar3DChart>` with `barDir=col`).
    Column3D,
    /// Line chart (`<c:lineChart>`).
    Line,
    /// 3D line chart (`<c:line3DChart>`).
    Line3D,
    /// Pie chart (`<c:pieChart>`).
    Pie,
    /// 3D pie chart (`<c:pie3DChart>`).
    Pie3D,
    /// Doughnut chart (`<c:doughnutChart>`).
    Doughnut,
    /// Pie-of-pie or bar-of-pie chart (`<c:ofPieChart>`).
    OfPie,
    /// Area chart (`<c:areaChart>`).
    Area,
    /// 3D area chart (`<c:area3DChart>`).
    Area3D,
    /// Scatter (XY) chart (`<c:scatterChart>`).
    Scatter,
    /// Bubble chart (`<c:bubbleChart>`).
    Bubble,
    /// Radar chart (`<c:radarChart>`).
    Radar,
    /// Surface chart (`<c:surfaceChart>`).
    Surface,
    /// 3D surface chart (`<c:surface3DChart>`).
    Surface3D,
    /// Stock chart (`<c:stockChart>`).
    Stock,
}

/// Chart data grouping mode.
///
/// Determines how multiple series are arranged visually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum ChartGrouping {
    /// Side-by-side bars/areas (default).
    #[default]
    Clustered,
    /// Stacked on top of each other.
    Stacked,
    /// Stacked to 100%.
    PercentStacked,
    /// Standard grouping (used by line/scatter).
    Standard,
}

/// Legend position relative to the chart area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum LegendPosition {
    /// Legend on the right side (default).
    #[default]
    Right,
    /// Legend at the bottom.
    Bottom,
    /// Legend at the top.
    Top,
    /// Legend on the left side.
    Left,
    /// No legend displayed.
    None,
}

/// Chart data — either category-based or XY-based.
///
/// # Examples
///
/// ```
/// use hwpforge_core::chart::ChartData;
///
/// let cat = ChartData::category(
///     &["A", "B"],
///     &[("Series1", &[10.0, 20.0])],
/// );
/// assert!(matches!(cat, ChartData::Category { .. }));
///
/// let xy = ChartData::xy(&[("Points", &[1.0, 2.0], &[3.0, 4.0])]);
/// assert!(matches!(xy, ChartData::Xy { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ChartData {
    /// Category-based data (bar, line, pie, area, radar, etc.).
    Category {
        /// Category labels (X-axis).
        categories: Vec<String>,
        /// Data series, each with a name and values.
        series: Vec<ChartSeries>,
    },
    /// XY-based data (scatter, bubble).
    Xy {
        /// XY series, each with name + x/y value arrays.
        series: Vec<XySeries>,
    },
}

/// A named data series with values aligned to categories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChartSeries {
    /// Series name (shown in legend).
    pub name: String,
    /// Numeric values (one per category).
    pub values: Vec<f64>,
}

/// A named XY data series (for scatter/bubble charts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct XySeries {
    /// Series name (shown in legend).
    pub name: String,
    /// X-axis values.
    pub x_values: Vec<f64>,
    /// Y-axis values (must be same length as `x_values`).
    pub y_values: Vec<f64>,
}

impl ChartData {
    /// Creates category-based chart data from slices.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::chart::ChartData;
    ///
    /// let data = ChartData::category(
    ///     &["Jan", "Feb", "Mar"],
    ///     &[("Revenue", &[100.0, 200.0, 300.0])],
    /// );
    /// match &data {
    ///     ChartData::Category { categories, series } => {
    ///         assert_eq!(categories.len(), 3);
    ///         assert_eq!(series.len(), 1);
    ///     }
    ///     _ => unreachable!(),
    /// }
    /// ```
    pub fn category(cats: &[&str], series: &[(&str, &[f64])]) -> Self {
        Self::Category {
            categories: cats.iter().map(|s| (*s).to_string()).collect(),
            series: series
                .iter()
                .map(|(name, vals)| ChartSeries {
                    name: (*name).to_string(),
                    values: vals.to_vec(),
                })
                .collect(),
        }
    }

    /// Creates XY-based chart data from slices.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_core::chart::ChartData;
    ///
    /// let data = ChartData::xy(&[("Points", &[1.0, 2.0], &[3.0, 4.0])]);
    /// match &data {
    ///     ChartData::Xy { series } => {
    ///         assert_eq!(series.len(), 1);
    ///         assert_eq!(series[0].x_values.len(), 2);
    ///     }
    ///     _ => unreachable!(),
    /// }
    /// ```
    pub fn xy(series: &[(&str, &[f64], &[f64])]) -> Self {
        Self::Xy {
            series: series
                .iter()
                .map(|(name, xs, ys)| XySeries {
                    name: (*name).to_string(),
                    x_values: xs.to_vec(),
                    y_values: ys.to_vec(),
                })
                .collect(),
        }
    }

    /// Returns `true` if any series contains data.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Category { series, .. } => series.is_empty(),
            Self::Xy { series } => series.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_type_all_18_variants() {
        let variants = [
            ChartType::Bar,
            ChartType::Column,
            ChartType::Bar3D,
            ChartType::Column3D,
            ChartType::Line,
            ChartType::Line3D,
            ChartType::Pie,
            ChartType::Pie3D,
            ChartType::Doughnut,
            ChartType::OfPie,
            ChartType::Area,
            ChartType::Area3D,
            ChartType::Scatter,
            ChartType::Bubble,
            ChartType::Radar,
            ChartType::Surface,
            ChartType::Surface3D,
            ChartType::Stock,
        ];
        assert_eq!(variants.len(), 18);
        // All distinct
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "variants {i} and {j} should be distinct");
                }
            }
        }
    }

    #[test]
    fn chart_data_category_convenience() {
        let data = ChartData::category(
            &["Q1", "Q2", "Q3", "Q4"],
            &[("Sales", &[100.0, 150.0, 200.0, 250.0]), ("Costs", &[80.0, 90.0, 100.0, 110.0])],
        );
        match &data {
            ChartData::Category { categories, series } => {
                assert_eq!(categories.len(), 4);
                assert_eq!(series.len(), 2);
                assert_eq!(series[0].name, "Sales");
                assert_eq!(series[1].values, &[80.0, 90.0, 100.0, 110.0]);
            }
            _ => panic!("expected Category"),
        }
    }

    #[test]
    fn chart_data_xy_convenience() {
        let data = ChartData::xy(&[("Points", &[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0])]);
        match &data {
            ChartData::Xy { series } => {
                assert_eq!(series.len(), 1);
                assert_eq!(series[0].name, "Points");
                assert_eq!(series[0].x_values, &[1.0, 2.0, 3.0]);
                assert_eq!(series[0].y_values, &[10.0, 20.0, 30.0]);
            }
            _ => panic!("expected Xy"),
        }
    }

    #[test]
    fn chart_data_is_empty() {
        let empty_cat = ChartData::category(&["A"], &[]);
        assert!(empty_cat.is_empty());

        let non_empty = ChartData::category(&["A"], &[("S", &[1.0])]);
        assert!(!non_empty.is_empty());

        let empty_xy = ChartData::Xy { series: vec![] };
        assert!(empty_xy.is_empty());
    }

    #[test]
    fn serde_roundtrip_chart_data() {
        let data = ChartData::category(&["A", "B"], &[("S1", &[1.0, 2.0])]);
        let json = serde_json::to_string(&data).unwrap();
        let back: ChartData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn serde_roundtrip_xy_data() {
        let data = ChartData::xy(&[("P", &[1.0, 2.0], &[3.0, 4.0])]);
        let json = serde_json::to_string(&data).unwrap();
        let back: ChartData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn chart_grouping_default() {
        assert_eq!(ChartGrouping::default(), ChartGrouping::Clustered);
    }

    #[test]
    fn legend_position_default() {
        assert_eq!(LegendPosition::default(), LegendPosition::Right);
    }

    #[test]
    fn chart_type_copy_clone() {
        let ct = ChartType::Pie;
        let ct2 = ct;
        assert_eq!(ct, ct2);
    }
}
