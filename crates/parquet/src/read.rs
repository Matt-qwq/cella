//! A Parquet file as rows: the read half of the connector.
//!
//! [`read_table`] turns the bytes of a `.parquet` file into a header-free grid of [`Value`]s plus
//! one name and one type per column. It is deliberately dumb: no header row is invented (that is
//! the query layer's `PromoteHeaders` step), dates are not turned into Excel serials, and nothing
//! is flattened. What the file holds is what the table holds.
//!
//! ## What is read
//!
//! | file column | cell |
//! |---|---|
//! | `BOOLEAN` | TRUE / FALSE |
//! | `INT32` | a number from the 32 bits, read as unsigned when the file says so |
//! | `INT64` | a number from the 64 bits (past 2^53 it loses digits, as every sheet number does) |
//! | `FLOAT`, `DOUBLE` | a number; NaN and the infinities become `#NUM!`, because a sheet holds neither |
//! | `BYTE_ARRAY`, `FIXED_LEN_BYTE_ARRAY` | text, decoded as UTF-8 with invalid sequences replaced — never an error |
//! | `INT32` with the `DATE` annotation | a number: the days since 1970-01-01, unconverted (a date *is* a number) |
//! | `DECIMAL` on any physical type | a number: the unscaled integer divided by 10^scale |
//!
//! `INT_8`/`INT_16`/unsigned annotations only decide the sign interpretation; `TIME` and
//! `TIMESTAMP` annotations are read as the plain number of their unit, since a time zone would be
//! a guess. A null — and any value below its column's maximum definition level — is
//! [`Value::Empty`].
//!
//! Several row groups, dictionary pages, plain and RLE pages, and the two codecs this crate
//! compiles in (uncompressed and Snappy) all read. Sizes are always taken from the file and
//! checked against the sheet's limits *before* they are used to reserve memory.
//!
//! ## What is refused, and why
//!
//! - **Nested or repeated columns** (`LIST`, `MAP`, a struct, a `REPEATED` field): a sheet cell is
//!   one value, and flattening a list into one cell would be a guess dressed up as data.
//! - **`INT96` timestamps**: a Julian day plus nanoseconds becomes a number only after choosing a
//!   time zone and an epoch, which changes the data.
//! - **A binary `DECIMAL` of more than 38 digits**: it does not fit the `i128` the conversion goes
//!   through, and Parquet's own maximum precision is 38, so such a column is malformed.
//! - **A compression codec this build does not compile in** (Gzip, Brotli, Zstd, Lz4, Lzo): the
//!   column is named in the error rather than failing deep in a page decoder.
//! - **More than [`MAX_ROWS`] rows or [`MAX_COLUMNS`] columns**: both are the sheet's own limits,
//!   and both are checked against the declared metadata before a row or a column is read.

use gridcraft_core::{CellError, Value};
use parquet::basic::{Compression, ConvertedType, LogicalType, Type as PhysicalType};
use parquet::column::reader::{ColumnReader, ColumnReaderImpl};
use parquet::data_type::{BoolType, ByteArrayType, DoubleType, FixedLenByteArray, FixedLenByteArrayType, FloatType, Int32Type, Int64Type};
use parquet::errors::ParquetError;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::schema::types::ColumnDescPtr;

use crate::ColumnType;

/// The sheet's row limit: a file with more rows than a sheet has cannot be put on one.
pub const MAX_ROWS: usize = gridcraft_core::MAX_ROWS as usize;
/// The sheet's column limit.
pub const MAX_COLUMNS: usize = gridcraft_core::MAX_COLS as usize;

/// The file marker Parquet writes at both ends of a file.
const MAGIC: &[u8] = b"PAR1";

/// Rows decoded per call into a column reader. Small enough that the levels and values of one
/// batch stay small whatever the file declares, large enough not to decode one row at a time.
const BATCH_ROWS: usize = 8_192;

/// A whole Parquet file, as a sheet would hold it.
#[derive(Clone, Debug, Default)]
pub struct Table {
    /// One name and one type per column, in the file's order.
    pub columns: Vec<(String, ColumnType)>,
    /// The file's rows, verbatim: no header row is added, and a null is `Value::Empty`.
    pub rows: Vec<Vec<Value>>,
}

/// Why a file could not be read. The `Display` text is what a person sees in a dialog, so it
/// names the limit, the column and the reason.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file has no bytes at all.
    #[error("this file is empty")]
    Empty,
    /// The bytes do not carry Parquet's file marker.
    #[error("this file is not a Parquet file (it does not begin and end with `PAR1`)")]
    NotParquet,
    /// The file has a group or a repeated column: not a flat table.
    #[error("this file has nested or repeated columns; GridCraft imports flat tables")]
    Nested,
    /// More rows than a sheet has.
    #[error("this file has {rows} rows; GridCraft imports at most {limit} (the sheet's row limit)")]
    TooManyRows {
        /// Rows the file declares.
        rows: u64,
        /// Rows a sheet has.
        limit: u64,
    },
    /// More columns than a sheet has.
    #[error("this file has {columns} columns; GridCraft imports at most {limit} (the sheet's column limit)")]
    TooManyColumns {
        /// Columns the file has.
        columns: usize,
        /// Columns a sheet has.
        limit: usize,
    },
    /// A column is compressed with a codec this build does not compile in.
    #[error("column {column} is compressed with {codec}; GridCraft reads uncompressed and Snappy files (re-save it as Snappy)")]
    Compression {
        /// The column's name.
        column: String,
        /// The codec as Parquet spells it.
        codec: String,
    },
    /// A column holds something this importer will not guess at.
    #[error("column {column} holds {kind}, which GridCraft does not import")]
    Unsupported {
        /// The column's name.
        column: String,
        /// What it holds, named the way it is stored.
        kind: &'static str,
    },
    /// The file's own metadata and data disagree.
    #[error("this file is damaged: {detail}")]
    Damaged {
        /// What does not add up.
        detail: String,
    },
    /// The Parquet crate refused something this importer does not check itself.
    #[error("this file could not be read as Parquet: {0}")]
    Parquet(#[from] ParquetError),
}

/// Reads a whole Parquet file into rows, with the column names from the file's schema.
///
/// The rows are the file's rows: the first row is data, not a header, and the caller decides
/// whether it means one.
pub fn read_table(bytes: &[u8]) -> Result<Table, ReadError> {
    if bytes.is_empty() {
        return Err(ReadError::Empty);
    }
    // Parquet brackets a file with its marker; a blob with neither end is not one, and a file cut
    // short has lost the trailing copy — which is also where the metadata lives.
    if bytes.len() < 2 * MAGIC.len() || !bytes.starts_with(MAGIC) || !bytes.ends_with(MAGIC) {
        return Err(ReadError::NotParquet);
    }
    // The reader owns its source (SerializedFileReader wants a 'static one), so the file is
    // copied once here and sliced from that copy from then on.
    let reader = SerializedFileReader::new(bytes::Bytes::copy_from_slice(bytes))?;
    let metadata = reader.metadata();
    let schema = metadata.file_metadata().schema_descr();

    // Flat only: a group field or a repeated column is a nested shape, and a sheet has none.
    if schema.root_schema().get_fields().iter().any(|field| !field.is_primitive()) || schema.columns().iter().any(|column| column.max_rep_level() > 0)
    {
        return Err(ReadError::Nested);
    }
    let leaves = schema.columns();
    if leaves.is_empty() {
        return Err(ReadError::Damaged { detail: "it has no columns".to_string() });
    }

    // Types first: a column this importer cannot read is refused before any row is read.
    let mut columns = Vec::with_capacity(leaves.len());
    let mut plans = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        let kind = kind_of(leaf)?;
        columns.push((leaf.name().to_string(), kind.column_type()));
        plans.push(ColumnPlan { kind, max_def: leaf.max_def_level() });
    }

    // Codecs: this build compiles Snappy and nothing else, so a Gzip file is refused by column
    // rather than failing with "codec not supported" halfway through a page.
    for group in metadata.row_groups() {
        for (index, chunk) in group.columns().iter().enumerate() {
            let codec = chunk.compression();
            if matches!(codec, Compression::UNCOMPRESSED | Compression::SNAPPY) {
                continue;
            }
            let column = columns.get(index).map(|(name, _)| name.clone()).unwrap_or_default();
            return Err(ReadError::Compression { column, codec: codec_name(codec) });
        }
    }

    // Row counts come from the file's footer, so check them before reserving a row.
    let mut declared: u64 = 0;
    for group in metadata.row_groups() {
        let rows = u64::try_from(group.num_rows())
            .map_err(|_| ReadError::Damaged { detail: "a row group declares a negative number of rows".to_string() })?;
        declared = declared.saturating_add(rows);
    }
    check_caps(declared, columns.len())?;

    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(declared.min(BATCH_ROWS as u64) as usize);
    for (index, group) in metadata.row_groups().iter().enumerate() {
        let count = usize::try_from(group.num_rows())
            .map_err(|_| ReadError::Damaged { detail: "a row group declares a negative number of rows".to_string() })?;
        let group_reader = reader.get_row_group(index)?;
        let mut decoded: Vec<Vec<Value>> = Vec::with_capacity(plans.len());
        for (column, plan) in plans.iter().enumerate() {
            let name = columns.get(column).map(|(name, _)| name.as_str()).unwrap_or_default();
            let column_reader = group_reader.get_column_reader(column)?;
            decoded.push(read_column(column_reader, *plan, count, name)?);
        }
        // Column-major to row-major, moving the values out instead of cloning them.
        let mut columns_of_values: Vec<std::vec::IntoIter<Value>> = decoded.into_iter().map(Vec::into_iter).collect();
        for _ in 0..count {
            let mut row = Vec::with_capacity(columns_of_values.len());
            for values in &mut columns_of_values {
                row.push(values.next().unwrap_or(Value::Empty));
            }
            rows.push(row);
        }
    }
    Ok(Table { columns, rows })
}

/// Refuses a file that is bigger than a sheet. Kept apart from reading so the limits can be
/// proved without writing a file that big.
pub(crate) fn check_caps(rows: u64, columns: usize) -> Result<(), ReadError> {
    if rows > MAX_ROWS as u64 {
        return Err(ReadError::TooManyRows { rows, limit: MAX_ROWS as u64 });
    }
    if columns > MAX_COLUMNS {
        return Err(ReadError::TooManyColumns { columns, limit: MAX_COLUMNS });
    }
    Ok(())
}

/// A column as this importer reads it: the conversion its values go through, and the definition
/// level that means "this row has a value".
#[derive(Clone, Copy, Debug)]
struct ColumnPlan {
    kind: Kind,
    max_def: i16,
}

/// How one column's values become cells.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Bool,
    /// `INT32` as a signed number.
    Int32,
    /// `INT64` as a signed number.
    Int64,
    /// `INT32` with an unsigned annotation, read as the bits it is.
    Unsigned32,
    /// `INT64` with an unsigned annotation, read as the bits it is.
    Unsigned64,
    Float,
    Double,
    /// `INT32` days since 1970-01-01, kept as the number the file holds.
    Date,
    /// A `DECIMAL`: the unscaled integer divided by 10^scale.
    Decimal {
        scale: i32,
    },
    /// Text (anything that is not a `DECIMAL`), read as lossily decoded UTF-8.
    Text,
}

impl Kind {
    /// The type the query layer sees for this column.
    fn column_type(self) -> ColumnType {
        match self {
            Kind::Bool => ColumnType::Bool,
            Kind::Int32 | Kind::Unsigned32 => ColumnType::Int32,
            Kind::Int64 | Kind::Unsigned64 => ColumnType::Int64,
            Kind::Float => ColumnType::Float,
            Kind::Double => ColumnType::Double,
            Kind::Date => ColumnType::Date,
            Kind::Decimal { .. } => ColumnType::Decimal,
            Kind::Text => ColumnType::Text,
        }
    }
}

/// The logical annotation that decides how a column's numbers are read.
#[derive(Clone, Copy, Debug)]
enum Annotation {
    None,
    Date,
    Decimal { scale: i32, precision: i32 },
    Unsigned,
}

/// The annotation on a column, preferring the modern `logicalType` over the old
/// `convertedType` when a writer set both. A `logicalType` that merely names a type the sheet
/// has no equivalent for reads as the plain number it is.
fn annotation(descr: &ColumnDescPtr) -> Annotation {
    match descr.logical_type() {
        Some(LogicalType::Date) => return Annotation::Date,
        Some(LogicalType::Decimal { scale, precision }) => return Annotation::Decimal { scale, precision },
        Some(LogicalType::Integer { is_signed: false, .. }) => return Annotation::Unsigned,
        _ => {}
    }
    match descr.converted_type() {
        ConvertedType::DATE => Annotation::Date,
        ConvertedType::DECIMAL => Annotation::Decimal { scale: descr.type_scale(), precision: descr.type_precision() },
        ConvertedType::UINT_8 | ConvertedType::UINT_16 | ConvertedType::UINT_32 | ConvertedType::UINT_64 => Annotation::Unsigned,
        _ => Annotation::None,
    }
}

/// How a column is read, or a refusal naming the column.
fn kind_of(descr: &ColumnDescPtr) -> Result<Kind, ReadError> {
    let unsupported = |kind: &'static str| ReadError::Unsupported { column: descr.name().to_string(), kind };
    Ok(match descr.physical_type() {
        PhysicalType::BOOLEAN => Kind::Bool,
        PhysicalType::INT32 => match annotation(descr) {
            Annotation::Date => Kind::Date,
            Annotation::Decimal { scale, .. } => Kind::Decimal { scale },
            Annotation::Unsigned => Kind::Unsigned32,
            Annotation::None => Kind::Int32,
        },
        PhysicalType::INT64 => match annotation(descr) {
            // A `DATE` is an `INT32`; a file that says otherwise still holds the number it holds.
            Annotation::Date => Kind::Int64,
            Annotation::Decimal { scale, .. } => Kind::Decimal { scale },
            Annotation::Unsigned => Kind::Unsigned64,
            Annotation::None => Kind::Int64,
        },
        PhysicalType::INT96 => return Err(unsupported("an INT96 timestamp")),
        PhysicalType::FLOAT => Kind::Float,
        PhysicalType::DOUBLE => Kind::Double,
        PhysicalType::BYTE_ARRAY | PhysicalType::FIXED_LEN_BYTE_ARRAY => match annotation(descr) {
            Annotation::Decimal { scale, precision } => {
                // `i128` holds 38 decimal digits, and 38 is Parquet's own maximum precision.
                if !(1..=38).contains(&precision) {
                    return Err(unsupported("a decimal of more than 38 digits"));
                }
                Kind::Decimal { scale }
            }
            _ => Kind::Text,
        },
    })
}

/// The codec as Parquet spells it, without the compression level.
pub(crate) fn codec_name(codec: Compression) -> String {
    match codec {
        Compression::UNCOMPRESSED => "UNCOMPRESSED".to_string(),
        Compression::SNAPPY => "SNAPPY".to_string(),
        Compression::GZIP(_) => "GZIP".to_string(),
        Compression::LZO => "LZO".to_string(),
        Compression::BROTLI(_) => "BROTLI".to_string(),
        Compression::LZ4 => "LZ4".to_string(),
        Compression::ZSTD(_) => "ZSTD".to_string(),
        Compression::LZ4_RAW => "LZ4_RAW".to_string(),
    }
}

/// Reads one whole column chunk of a row group into cells, nulls included.
fn read_column(reader: ColumnReader, plan: ColumnPlan, rows: usize, column: &str) -> Result<Vec<Value>, ReadError> {
    match reader {
        ColumnReader::BoolColumnReader(reader) => read_bool_column(reader, plan, rows),
        ColumnReader::Int32ColumnReader(reader) => read_int32_column(reader, plan, rows),
        ColumnReader::Int64ColumnReader(reader) => read_int64_column(reader, plan, rows),
        ColumnReader::FloatColumnReader(reader) => read_float_column(reader, plan, rows),
        ColumnReader::DoubleColumnReader(reader) => read_double_column(reader, plan, rows),
        ColumnReader::ByteArrayColumnReader(reader) => read_byte_array_column(reader, plan, rows),
        ColumnReader::FixedLenByteArrayColumnReader(reader) => read_fixed_column(reader, plan, rows),
        // `kind_of` refuses these columns before a row group is opened; the arm keeps the match
        // exhaustive without an `unreachable!`.
        ColumnReader::Int96ColumnReader(_) => Err(ReadError::Unsupported { column: column.to_string(), kind: "an INT96 timestamp" }),
    }
}

/// One reader per physical type: decode `rows` cells, placing a `Value::Empty` wherever the
/// definition level says the row has no value.
macro_rules! typed_column {
    ($name:ident, $marker:ty, $native:ty, $convert:path) => {
        fn $name(mut reader: ColumnReaderImpl<$marker>, plan: ColumnPlan, rows: usize) -> Result<Vec<Value>, ReadError> {
            let mut out: Vec<Value> = Vec::with_capacity(rows);
            let mut levels: Vec<i16> = Vec::new();
            let mut values: Vec<$native> = Vec::new();
            while out.len() < rows {
                let want = BATCH_ROWS.min(rows - out.len());
                levels.clear();
                values.clear();
                let (records, _, _) = reader.read_records(want, Some(&mut levels), None, &mut values)?;
                if records == 0 {
                    break;
                }
                if plan.max_def == 0 {
                    // A required column has no definition levels: every record is a value.
                    for value in &values {
                        out.push($convert(&plan, value));
                    }
                } else {
                    let mut present = values.iter();
                    for level in &levels {
                        match present.next() {
                            Some(value) if *level >= plan.max_def => out.push($convert(&plan, value)),
                            _ => out.push(Value::Empty),
                        }
                    }
                }
            }
            if out.len() != rows {
                return Err(ReadError::Damaged { detail: format!("a column holds {} values where its row group declares {rows}", out.len()) });
            }
            // More values than declared would be rows silently lost, so look for one.
            levels.clear();
            values.clear();
            let (extra, _, _) = reader.read_records(1, Some(&mut levels), None, &mut values)?;
            if extra > 0 {
                return Err(ReadError::Damaged { detail: format!("a column holds more rows than its row group declares ({rows})") });
            }
            Ok(out)
        }
    };
}

typed_column!(read_bool_column, BoolType, bool, bool_cell);
typed_column!(read_int32_column, Int32Type, i32, int32_cell);
typed_column!(read_int64_column, Int64Type, i64, int64_cell);
typed_column!(read_float_column, FloatType, f32, float_cell);
typed_column!(read_double_column, DoubleType, f64, double_cell);
typed_column!(read_byte_array_column, ByteArrayType, parquet::data_type::ByteArray, byte_array_cell);
typed_column!(read_fixed_column, FixedLenByteArrayType, FixedLenByteArray, fixed_cell);

fn bool_cell(_plan: &ColumnPlan, value: &bool) -> Value {
    Value::Bool(*value)
}

fn int32_cell(plan: &ColumnPlan, value: &i32) -> Value {
    match plan.kind {
        Kind::Unsigned32 => Value::Number(f64::from(*value as u32)),
        Kind::Decimal { scale } => scaled(f64::from(*value), scale),
        _ => Value::Number(f64::from(*value)),
    }
}

fn int64_cell(plan: &ColumnPlan, value: &i64) -> Value {
    match plan.kind {
        Kind::Unsigned64 => Value::Number(*value as u64 as f64),
        Kind::Decimal { scale } => scaled(*value as f64, scale),
        _ => Value::Number(*value as f64),
    }
}

fn float_cell(_plan: &ColumnPlan, value: &f32) -> Value {
    Value::number(f64::from(*value))
}

fn double_cell(_plan: &ColumnPlan, value: &f64) -> Value {
    // `Value::number` turns NaN and the infinities into `#NUM!`: a sheet holds neither.
    Value::number(*value)
}

fn byte_array_cell(plan: &ColumnPlan, value: &parquet::data_type::ByteArray) -> Value {
    byte_cell(plan, value.data())
}

fn fixed_cell(plan: &ColumnPlan, value: &FixedLenByteArray) -> Value {
    byte_cell(plan, value.data())
}

/// A byte payload as a cell: a `DECIMAL` is scaled, anything else becomes text.
fn byte_cell(plan: &ColumnPlan, bytes: &[u8]) -> Value {
    match plan.kind {
        Kind::Decimal { scale } => decimal(bytes, scale),
        _ => text(bytes),
    }
}

/// Text from bytes. A `BYTE_ARRAY` is not required to be UTF-8, and one file's odd bytes are not
/// a reason to fail a whole import: they are replaced, the way every lossy reader does it.
fn text(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => Value::text(text),
        Err(_) => Value::text(String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// A binary `DECIMAL`: big-endian two's complement of the unscaled value, scaled by 10^-scale.
fn decimal(bytes: &[u8], scale: i32) -> Value {
    let mut unscaled: i128 = if bytes.first().is_some_and(|byte| *byte & 0x80 != 0) { -1 } else { 0 };
    for byte in bytes {
        let Some(next) = unscaled.checked_mul(256).and_then(|value| value.checked_add(i128::from(*byte))) else {
            // Past 38 digits: a number no sheet can hold either.
            return Value::Error(CellError::Num);
        };
        unscaled = next;
    }
    scaled(unscaled as f64, scale)
}

/// `unscaled / 10^scale`, with a scale that cannot produce a number falling back to the integer.
fn scaled(unscaled: f64, scale: i32) -> Value {
    let divisor = 10f64.powi(scale);
    Value::number(if divisor.is_finite() && divisor != 0.0 { unscaled / divisor } else { unscaled })
}
