//! The roads from a GeoParquet file: every column a property (as GeoPandas' GeoJSON writes it), the geometry a LineString from WKB.

use arrow_array::{cast::AsArray, types::*, Array, ArrayRef};
use arrow_schema::DataType;
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde_json::{Map, Value};

pub struct Feature {
    pub props: Map<String, Value>,
    /// None: not a LineString (the stages skip it, as roadstyle does)
    pub coords: Option<Vec<[f64; 2]>>,
}

pub fn read_roads(data: Vec<u8>, geom_col: &str) -> Result<Vec<Feature>, String> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data))
        .and_then(|b| b.build())
        .map_err(|e| format!("not a Parquet file: {e}"))?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| e.to_string())?;
        let schema = batch.schema();
        let start = out.len();
        out.extend((0..batch.num_rows()).map(|_| Feature { props: Map::new(), coords: None }));
        for (field, col) in schema.fields().iter().zip(batch.columns()) {
            for (r, f) in out[start..].iter_mut().enumerate() {
                if field.name() == geom_col {
                    f.coords = binary(col, r).and_then(wkb_line);
                } else {
                    f.props.insert(field.name().clone(), value(col, r)?);
                }
            }
        }
    }
    Ok(out)
}

/// Any Parquet table as rows: the columns as properties, the binary column `blob` kept as its bytes.
pub fn read_table(data: Vec<u8>, blob: &str) -> Result<Vec<(Map<String, Value>, Option<Vec<u8>>)>, String> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(data))
        .and_then(|b| b.build())
        .map_err(|e| format!("not a Parquet file: {e}"))?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| e.to_string())?;
        let schema = batch.schema();
        let start = out.len();
        out.extend((0..batch.num_rows()).map(|_| (Map::new(), None)));
        for (field, col) in schema.fields().iter().zip(batch.columns()) {
            for (r, row) in out[start..].iter_mut().enumerate() {
                if field.name() == blob {
                    row.1 = binary(col, r).map(<[u8]>::to_vec);
                } else {
                    row.0.insert(field.name().clone(), value(col, r)?);
                }
            }
        }
    }
    Ok(out)
}

fn value(col: &ArrayRef, r: usize) -> Result<Value, String> {
    if col.is_null(r) {
        return Ok(Value::Null);
    }
    Ok(match col.data_type() {
        DataType::Null => Value::Null, // a column that is null in every row (pyarrow writes it so)
        DataType::Utf8 => col.as_string::<i32>().value(r).into(),
        DataType::LargeUtf8 => col.as_string::<i64>().value(r).into(),
        DataType::Utf8View => col.as_string_view().value(r).into(),
        DataType::Boolean => col.as_boolean().value(r).into(),
        DataType::Int32 => col.as_primitive::<Int32Type>().value(r).into(),
        DataType::Int64 => col.as_primitive::<Int64Type>().value(r).into(),
        DataType::Float32 => float(col.as_primitive::<Float32Type>().value(r) as f64),
        DataType::Float64 => float(col.as_primitive::<Float64Type>().value(r)),
        t => return Err(format!("column type {t} is not read")),
    })
}

/// A float as GeoPandas writes it: NaN is null.
fn float(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

fn binary(col: &ArrayRef, r: usize) -> Option<&[u8]> {
    if col.is_null(r) {
        return None;
    }
    match col.data_type() {
        DataType::Binary => Some(col.as_binary::<i32>().value(r)),
        DataType::LargeBinary => Some(col.as_binary::<i64>().value(r)),
        DataType::BinaryView => Some(col.as_binary_view().value(r)),
        _ => None,
    }
}

/// The points of a 2D WKB LineString; None for any other geometry.
fn wkb_line(b: &[u8]) -> Option<Vec<[f64; 2]>> {
    let le = *b.first()? == 1;
    let u32_at = |i: usize| -> Option<u32> {
        let w: [u8; 4] = b.get(i..i + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(w) } else { u32::from_be_bytes(w) })
    };
    let f64_at = |i: usize| -> Option<f64> {
        let w: [u8; 8] = b.get(i..i + 8)?.try_into().ok()?;
        Some(if le { f64::from_le_bytes(w) } else { f64::from_be_bytes(w) })
    };
    if u32_at(1)? != 2 {
        return None;
    }
    let n = u32_at(5)? as usize;
    (0..n).map(|k| Some([f64_at(9 + 16 * k)?, f64_at(17 + 16 * k)?])).collect()
}
