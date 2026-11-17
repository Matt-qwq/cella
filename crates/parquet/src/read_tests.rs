//! The reader, proved against the writer: a sheet goes out as a file and comes back as the same
//! rows. Every fixture is built in code — a binary file in the repository would be an asset to
//! attribute, and a test that reads a hand-made byte array proves the byte array, not the reader.

use std::sync::Arc;

use cella_core::{CellError, CellRef, Value};
use cella_model::{Cell, Sheet, Workbook};
use parquet::basic::{BrotliLevel, Compression, GzipLevel, LogicalType, Repetition, Type as PhysicalType, ZstdLevel};
use parquet::data_type::{FixedLenByteArray, Int32Type, Int64Type, Int96, Int96Type};
use parquet::file::properties::WriterProperties;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::types::Type;

use super::*;
use crate::read::{check_caps, codec_name};

/// A sheet holding `values`, cloned out of a workbook the way the export tests build one.
fn sheet_with(values: &[(u32, u32, Value)]) -> Sheet {
    let mut wb = Workbook::new();
    let sheet = wb.sheet_mut(0).expect("sheet");
    for (row, col, value) in values {
        sheet.cells.set(CellRef::new(*row, *col), Cell::value(value.clone()));
    }
    wb.sheet(0).expect("sheet").clone()
}

/// Writes a file with `fields` and runs `|group| { .. }` with the row group writer, so a test can
/// build the shapes the sheet exporter never makes (annotated numbers, INT96, nested groups).
macro_rules! parquet_file {
    ($fields:expr, |$group:ident| $body:block) => {{
        let schema = Type::group_type_builder("schema")
            .with_fields($fields.into_iter().map(Arc::new).collect())
            .build()
            .expect("schema");
        let mut bytes = Vec::new();
        let mut writer = SerializedFileWriter::new(&mut bytes, Arc::new(schema), Arc::new(WriterProperties::builder().build())).expect("writer");
        {
            let mut $group = writer.next_row_group().expect("row group");
            $body
            $group.close().expect("row group");
        }
        writer.close().expect("file");
        bytes
    }};
}

fn optional(name: &str, physical: PhysicalType) -> parquet::schema::types::PrimitiveTypeBuilder<'_> {
    Type::primitive_type_builder(name, physical).with_repetition(Repetition::OPTIONAL)
}

#[test]
fn every_column_type_the_exporter_writes_comes_back() {
    let sheet = sheet_with(&[
        (0, 0, Value::text("region")),
        (0, 1, Value::text("units")),
        (0, 2, Value::text("price")),
        (0, 3, Value::text("active")),
        (0, 4, Value::text("note")),
        (1, 0, Value::text("North")),
        (1, 1, Value::Number(12.0)),
        (1, 2, Value::Number(3.5)),
        (1, 3, Value::Bool(true)),
        (1, 4, Value::text("ok")),
        (2, 0, Value::text("South")),
        (2, 1, Value::Number(7.0)),
        (2, 3, Value::Bool(false)),
        (2, 4, Value::Error(CellError::NA)),
    ]);
    let (bytes, report) = export_sheet(&sheet, Options::default()).expect("export");
    let table = read_table(&bytes).expect("read back");

    let names: Vec<&str> = table.columns.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["region", "units", "price", "active", "note"]);
    let types: Vec<&str> = table.columns.iter().map(|(_, kind)| kind.as_str()).collect();
    assert_eq!(types, ["STRING", "INT64", "DOUBLE", "BOOLEAN", "STRING"]);
    assert_eq!(table.rows.len(), report.rows as usize, "the header row is data, not a header");

    let text = Value::text;
    assert_eq!(
        table.rows,
        vec![
            vec![text("North"), Value::Number(12.0), Value::Number(3.5), Value::Bool(true), text("ok")],
            // The blank cell is a null, and the #N/A was written as the text the cell shows.
            vec![text("South"), Value::Number(7.0), Value::Empty, Value::Bool(false), text("#N/A")],
        ]
    );
}

#[test]
fn two_row_groups_come_back_in_order() {
    // More rows than one row group holds, so the writer has to start a second one.
    let rows = ROW_GROUP_MAX_ROWS + 100;
    let mut values = vec![(0u32, 0u32, Value::text("n"))];
    for row in 1..=rows as u32 {
        values.push((row, 0, Value::Number(f64::from(row))));
    }
    let sheet = sheet_with(&values);
    let (bytes, _) = export_sheet(&sheet, Options::default()).expect("export");

    let reader = SerializedFileReader::new(bytes::Bytes::from(bytes.clone())).expect("read");
    assert!(reader.metadata().num_row_groups() > 1, "one row group, so the chunking never ran");

    let table = read_table(&bytes).expect("read back");
    assert_eq!(table.rows.len(), rows);
    assert_eq!(table.rows.first().and_then(|row| row.first().cloned()), Some(Value::Number(1.0)));
    assert_eq!(table.rows.last().and_then(|row| row.first().cloned()), Some(Value::Number(rows as f64)));
}

#[test]
fn an_empty_slice_is_refused_as_empty() {
    let error = read_table(&[]).expect_err("no bytes");
    assert!(matches!(error, ReadError::Empty));
    assert_eq!(error.to_string(), "this file is empty");
}

#[test]
fn a_blob_that_is_not_parquet_is_refused() {
    let error = read_table(&vec![b'x'; 4_096]).expect_err("not parquet");
    assert!(matches!(error, ReadError::NotParquet));
    assert!(error.to_string().contains("not a Parquet file"), "{error}");
}

#[test]
fn a_truncated_or_corrupted_file_is_refused() {
    let sheet = sheet_with(&[(0, 0, Value::text("a")), (1, 0, Value::Number(1.0)), (2, 0, Value::Number(2.0))]);
    let (bytes, _) = export_sheet(&sheet, Options::default()).expect("export");

    // Cut in half: the trailing marker — and the metadata it precedes — are gone.
    assert!(read_table(&bytes[..bytes.len() / 2]).is_err());
    assert!(matches!(read_table(&bytes[..bytes.len() - 4]), Err(ReadError::NotParquet)));

    // Both markers intact but the middle destroyed: the Parquet reader refuses the pages.
    let mut corrupted = bytes.clone();
    for byte in corrupted.iter_mut().skip(8).take(bytes.len() / 4) {
        *byte = 0;
    }
    assert!(read_table(&corrupted).is_err());
}

#[test]
fn nested_columns_are_refused_rather_than_flattened() {
    let leaf = Type::primitive_type_builder("a", PhysicalType::INT32).with_repetition(Repetition::REQUIRED).build().expect("leaf");
    let nested = Type::group_type_builder("nested").with_repetition(Repetition::REQUIRED).with_fields(vec![Arc::new(leaf)]).build().expect("group");
    let bytes = parquet_file!(vec![nested], |group| {
        while let Some(mut column) = group.next_column().expect("column") {
            column.typed::<Int32Type>().write_batch(&[7, 8], None, None).expect("values");
            column.close().expect("close");
        }
    });

    let error = read_table(&bytes).expect_err("nested");
    assert!(matches!(error, ReadError::Nested));
    assert!(error.to_string().contains("nested or repeated columns"), "{error}");
}

#[test]
fn dates_decimals_and_unsigned_integers_come_back_as_their_numbers() {
    let day = optional("day", PhysicalType::INT32).with_logical_type(Some(LogicalType::Date)).build().expect("day");
    let money = optional("money", PhysicalType::INT32)
        .with_precision(9)
        .with_scale(2)
        .with_logical_type(Some(LogicalType::Decimal { scale: 2, precision: 9 }))
        .build()
        .expect("money");
    let big = optional("big", PhysicalType::INT64)
        .with_precision(12)
        .with_scale(3)
        .with_logical_type(Some(LogicalType::Decimal { scale: 3, precision: 12 }))
        .build()
        .expect("big");
    let cents = optional("cents", PhysicalType::FIXED_LEN_BYTE_ARRAY)
        .with_length(4)
        .with_precision(7)
        .with_scale(2)
        .with_logical_type(Some(LogicalType::Decimal { scale: 2, precision: 7 }))
        .build()
        .expect("cents");
    let count = optional("count", PhysicalType::INT32)
        .with_logical_type(Some(LogicalType::Integer { bit_width: 32, is_signed: false }))
        .build()
        .expect("count");

    let bytes = parquet_file!(vec![day, money, big, cents, count], |group| {
        let mut column = group.next_column().expect("column").expect("day");
        column.typed::<Int32Type>().write_batch(&[20_000, 20_001], Some(&[1, 1]), None).expect("day");
        column.close().expect("close");

        let mut column = group.next_column().expect("column").expect("money");
        column.typed::<Int32Type>().write_batch(&[12_345, -5], Some(&[1, 1]), None).expect("money");
        column.close().expect("close");

        let mut column = group.next_column().expect("column").expect("big");
        column.typed::<Int64Type>().write_batch(&[12_345, -1_000], Some(&[1, 1]), None).expect("big");
        column.close().expect("close");

        let mut column = group.next_column().expect("column").expect("cents");
        let twelve_thousandths = FixedLenByteArray::from(vec![0x00, 0x00, 0x30, 0x39]);
        let minus_one = FixedLenByteArray::from(vec![0xFF, 0xFF, 0xFF, 0xFF]);
        column
            .typed::<parquet::data_type::FixedLenByteArrayType>()
            .write_batch(&[twelve_thousandths, minus_one], Some(&[1, 1]), None)
            .expect("cents");
        column.close().expect("close");

        let mut column = group.next_column().expect("column").expect("count");
        column.typed::<Int32Type>().write_batch(&[-1, 5], Some(&[1, 1]), None).expect("count");
        column.close().expect("close");
    });

    let table = read_table(&bytes).expect("read back");
    let types: Vec<&str> = table.columns.iter().map(|(_, kind)| kind.as_str()).collect();
    assert_eq!(types, ["DATE", "DECIMAL", "DECIMAL", "DECIMAL", "INT32"]);
    assert_eq!(
        table.rows,
        vec![
            // A date stays the number of days the file holds; a decimal is the scaled integer;
            // UINT_32 -1 is the 4294967295 the bits spell.
            vec![Value::Number(20_000.0), Value::Number(123.45), Value::Number(12.345), Value::Number(123.45), Value::Number(4_294_967_295.0)],
            vec![Value::Number(20_001.0), Value::Number(-0.05), Value::Number(-1.0), Value::Number(-0.01), Value::Number(5.0)],
        ]
    );
}

#[test]
fn an_int96_timestamp_is_refused_by_column_name() {
    let stamp = optional("stamp", PhysicalType::INT96).build().expect("stamp");
    let bytes = parquet_file!(vec![stamp], |group| {
        let mut column = group.next_column().expect("column").expect("stamp");
        let mut value = Int96::new();
        value.set_data(1, 2, 3);
        column.typed::<Int96Type>().write_batch(&[value], Some(&[1]), None).expect("values");
        column.close().expect("close");
    });

    let error = read_table(&bytes).expect_err("int96");
    assert!(matches!(error, ReadError::Unsupported { .. }));
    assert!(error.to_string().contains("stamp") && error.to_string().contains("INT96"), "{error}");
}

#[test]
fn files_bigger_than_a_sheet_are_refused_with_the_limit_in_the_message() {
    // The caps are checked against the file's declared counts before a row is read, so the guard
    // is tested by calling it with the smallest counts that trip it. Writing a file of 1,048,577
    // rows to prove the same thing would cost a gigabyte of test memory.
    assert!(check_caps(MAX_ROWS as u64, MAX_COLUMNS).is_ok(), "the limits themselves are allowed");

    let rows = check_caps(MAX_ROWS as u64 + 1, 1).expect_err("one row past the limit");
    assert!(matches!(rows, ReadError::TooManyRows { limit, .. } if limit == MAX_ROWS as u64));
    assert!(rows.to_string().contains("1048577") && rows.to_string().contains("row limit"), "{rows}");

    let columns = check_caps(1, MAX_COLUMNS + 1).expect_err("one column past the limit");
    assert!(matches!(columns, ReadError::TooManyColumns { limit, .. } if limit == MAX_COLUMNS));
    assert!(columns.to_string().contains("16385") && columns.to_string().contains("column limit"), "{columns}");
}

#[test]
fn a_codec_this_build_does_not_compile_in_is_named_without_its_level() {
    // Only uncompressed and Snappy are compiled in, so a Gzip or Zstd file is refused by column;
    // the message names the codec, never a compression level a person cannot act on.
    assert_eq!(codec_name(Compression::UNCOMPRESSED), "UNCOMPRESSED");
    assert_eq!(codec_name(Compression::SNAPPY), "SNAPPY");
    assert_eq!(codec_name(Compression::GZIP(GzipLevel::default())), "GZIP");
    assert_eq!(codec_name(Compression::BROTLI(BrotliLevel::default())), "BROTLI");
    assert_eq!(codec_name(Compression::ZSTD(ZstdLevel::default())), "ZSTD");
    assert_eq!(codec_name(Compression::LZ4), "LZ4");
    assert_eq!(codec_name(Compression::LZ4_RAW), "LZ4_RAW");
    assert_eq!(codec_name(Compression::LZO), "LZO");
}
