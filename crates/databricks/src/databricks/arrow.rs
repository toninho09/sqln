//! Reads the Arrow stream of one result part into rows of the grid. Each value keeps its
//! original text: numbers stay numbers, bytes stay bytes, and `NULL` stays `NULL`.

use std::fmt::Write;
use std::io::Cursor;

use arrow_array::cast::AsArray;
use arrow_array::{Array, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_ipc::reader::StreamReader;
use arrow_schema::DataType;

use crate::connector::ExecError;
use crate::grid::{Binary, Row, RowWriter, Value};
use crate::text;

fn decode_error() -> ExecError {
    ExecError::new(text::DATABRICKS_BAD_ANSWER)
}

/// Returns the record batches of one result part.
pub fn batches(bytes: Vec<u8>) -> Result<Vec<RecordBatch>, ExecError> {
    let reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(|_| decode_error())?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| decode_error())
}

/// Converts the rows of a batch.
pub fn rows(batch: &RecordBatch) -> Result<Vec<Row>, ExecError> {
    let options = FormatOptions::default();
    let columns = batch.columns();
    let formatters = columns
        .iter()
        .map(|column| ArrayFormatter::try_new(column.as_ref(), &options))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| decode_error())?;
    let mut writer = RowWriter::default();
    let mut buffer = String::new();
    let mut rows = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        for (column, formatter) in columns.iter().zip(&formatters) {
            push(&mut writer, column.as_ref(), formatter, row, &mut buffer)?;
        }
        rows.push(writer.finish());
    }
    Ok(rows)
}

fn push(
    writer: &mut RowWriter,
    column: &dyn Array,
    formatter: &ArrayFormatter<'_>,
    row: usize,
    buffer: &mut String,
) -> Result<(), ExecError> {
    if column.is_null(row) {
        writer.push(Value::Null);
        return Ok(());
    }
    match column.data_type() {
        DataType::Utf8 => writer.push(Value::Text(column.as_string::<i32>().value(row))),
        DataType::LargeUtf8 => writer.push(Value::Text(column.as_string::<i64>().value(row))),
        DataType::Utf8View => writer.push(Value::Text(column.as_string_view().value(row))),
        DataType::Binary => writer.push(Value::Binary(Binary::new(
            column.as_binary::<i32>().value(row),
        ))),
        DataType::LargeBinary => writer.push(Value::Binary(Binary::new(
            column.as_binary::<i64>().value(row),
        ))),
        DataType::BinaryView => {
            writer.push(Value::Binary(Binary::new(
                column.as_binary_view().value(row),
            )));
        }
        DataType::FixedSizeBinary(_) => writer.push(Value::Binary(Binary::new(
            column.as_fixed_size_binary().value(row),
        ))),
        data_type => {
            buffer.clear();
            write!(buffer, "{}", formatter.value(row)).map_err(|_| decode_error())?;
            writer.push(if data_type.is_numeric() {
                Value::Number(buffer)
            } else {
                Value::Text(buffer)
            });
        }
    }
    Ok(())
}

/// Converts a batch to text cells, for the catalog reads.
pub fn text_rows(batch: &RecordBatch) -> Result<Vec<Vec<Option<String>>>, ExecError> {
    Ok(rows(batch)?
        .iter()
        .map(|row| (0..row.columns()).map(|column| row.text(column)).collect())
        .collect())
}
