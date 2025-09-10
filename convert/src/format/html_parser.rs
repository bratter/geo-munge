use quick_xml::{events::Event, Reader};

use super::{Properties, Value};

enum TableType {
    KeyValue,
    RowWise,
    Unknown,
}

/// Parse HTML content to extract structured data from tables and lists.
///
/// This function attempts to extract structured data from common HTML patterns while being
/// permissive with malformed HTML. It uses a single-pass parser that stops at the first
/// matching pattern found.
///
/// ## Supported Patterns
///
/// - **Key-Value Tables**: 2-column tables where first column contains keys and second contains values.
///   Supports single or multiple rows. Returns `KeyValueTable` as a JSON object.
/// - **Row-wise Tables**: Tables with headers in first row and data in subsequent rows.
///   Requires 2+ rows. Returns `RowTable` as array of JSON objects.
/// - **Lists**: `<ul>` or `<ol>` elements. Returns `List` as array of strings.
///
/// All patterns will be returned as an appropriate [`Value`].
///
/// ## Parsing Behavior
///
/// - **Single-pass**: Scans HTML sequentially and returns first valid pattern found
/// - **Position-agnostic**: Finds tables/lists anywhere in the HTML document  
/// - **Permissive with boundaries**: Extracts complete elements only - requires closing tags
///   (`</tr>`, `</li>`) but gracefully handles malformed HTML by ignoring incomplete elements
/// - **Text flattening**: Extracts text from nested formatting tags (`<b>`, `<i>`, `<em>`,
///   `<strong>`, `<span>`, `<a>`, `<code>`) while preserving content
/// - **Fast-fail on complex tables**: Returns `None` immediately if `colspan` or `rowspan`
///   attributes are detected (which would break our parsing assumptions)
/// - **Single-level parsing**: Only processes the first table/list found, ignores nested structures
/// - **Original preservation**: Caller retains original HTML string regardless of parsing success
///
/// ## Examples
///
/// ```rust
/// // Key-value table (single row)
/// let html = r#"<table><tr><td>Status</td><td>Active</td></tr></table>"#;
/// // Returns: Some(Value::Object({"Status": "Active"}))
///
/// // List with mixed content
/// let html = r#"<p>Items:</p><ul><li>First</li><li>Second</li></ul>"#;
/// // Returns: Some(Value::Array(["First", "Second"]))
///
/// // Text flattening with formatting tags
/// let html = r#"<table><tr><td>Name</td><td>John <i>middle</i> Doe</td></tr></table>"#;
/// // Returns: Some(Value::Object({"Name": "John middle Doe"}))
///
/// // Malformed HTML - extracts what it can
/// let html = r#"<ul><li>Complete</li><li>Incomplete"#;
/// // Returns: Some(Value::Array(["Complete"]))
/// ```
pub fn parse_html(html: &str) -> Option<Value> {
    let mut reader = Reader::from_str(html);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"table" => {
                    if let Some(result) = parse_table(&mut reader, &mut buf) {
                        return Some(result);
                    }
                }
                b"ul" | b"ol" => {
                    if let Some(result) = parse_list(&mut reader, &mut buf) {
                        return Some(result);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

fn parse_table(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Option<Value> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut in_cell = false;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"td" | b"th" => {
                    // Check for colspan/rowspan attributes - fail fast if found
                    for attr_result in e.attributes() {
                        if let Ok(attr) = attr_result {
                            if matches!(attr.key.as_ref(), b"colspan" | b"rowspan") {
                                return None; // Fail fast on colspan/rowspan
                            }
                        }
                    }
                    in_cell = true;
                    current_cell.clear();
                }
                // Allow formatting tags - their text will be captured
                b"b" | b"i" | b"em" | b"strong" | b"span" | b"a" | b"code" => {
                    // Text from these elements will flow through to current_cell
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"table" => break, // Exit immediately on table close
                b"td" | b"th" => {
                    if in_cell {
                        current_row.push(current_cell.trim().to_string());
                        current_cell.clear();
                        in_cell = false;
                    }
                }
                b"tr" => {
                    if !current_row.is_empty() {
                        rows.push(std::mem::take(&mut current_row));
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_cell {
                    current_cell.push_str(&e.unescape().unwrap_or_default());
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    // Don't add remaining incomplete row - only include well-formed elements

    if rows.is_empty() {
        return None;
    }

    convert_table_to_structured_data(rows)
}

fn parse_list(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Option<Value> {
    let mut items = Vec::new();
    let mut current_item = String::new();
    let mut in_item = false;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"li" => {
                    in_item = true;
                    current_item.clear();
                }
                // Allow formatting tags - their text will be captured
                b"b" | b"i" | b"em" | b"strong" | b"span" | b"a" | b"code" => {
                    // Text from these elements will flow through to current_item
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"ul" | b"ol" => break, // Exit immediately on list close
                b"li" => {
                    if in_item {
                        let trimmed = current_item.trim();
                        if !trimmed.is_empty() {
                            items.push(Value::String(trimmed.to_string()));
                        }
                        current_item.clear();
                        in_item = false;
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_item {
                    current_item.push_str(&e.unescape().unwrap_or_default());
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    // Don't add remaining incomplete item - only include well-formed elements

    if items.is_empty() {
        None
    } else {
        Some(Value::Array(items))
    }
}

fn convert_table_to_structured_data(rows: Vec<Vec<String>>) -> Option<Value> {
    let table_type = analyze_table_structure(&rows);

    match table_type {
        TableType::KeyValue => convert_key_value_table(rows),
        TableType::RowWise => convert_row_table(rows),
        TableType::Unknown => None,
    }
}

fn analyze_table_structure(rows: &[Vec<String>]) -> TableType {
    if rows.is_empty() {
        return TableType::Unknown;
    }

    // Check for key-value pattern: exactly 2 columns, any number of rows >= 1
    if rows.iter().all(|row| row.len() == 2) && !rows.is_empty() {
        // Additional heuristic: first column should have unique values (keys)
        let first_column: Vec<&String> = rows.iter().map(|row| &row[0]).collect();
        let unique_count = first_column
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();

        if unique_count == first_column.len() {
            return TableType::KeyValue;
        }
    }

    // Check for row-wise pattern: consistent column count, 2+ rows (need headers + data)
    if rows.len() > 1 {
        let first_row_len = rows[0].len();
        if first_row_len > 0 && rows.iter().all(|row| row.len() == first_row_len) {
            return TableType::RowWise;
        }
    }

    TableType::Unknown
}

fn convert_key_value_table(rows: Vec<Vec<String>>) -> Option<Value> {
    let mut map = Properties::new();

    for row in rows {
        if row.len() == 2 {
            let key = row[0].trim().to_string();
            let value = row[1].trim().to_string();

            if !key.is_empty() {
                map.insert(key, Value::String(value));
            }
        }
    }

    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn convert_row_table(rows: Vec<Vec<String>>) -> Option<Value> {
    if rows.len() < 2 {
        return None;
    }

    let headers = &rows[0];
    let data_rows = &rows[1..];

    let mut result = Vec::new();

    for data_row in data_rows {
        let mut row_obj = Properties::new();

        for (i, value) in data_row.iter().enumerate() {
            if let Some(header) = headers.get(i) {
                let header = header.trim();
                let value = value.trim();

                if !header.is_empty() {
                    row_obj.insert(header.to_string(), Value::String(value.to_string()));
                }
            }
        }

        if !row_obj.is_empty() {
            result.push(Value::Object(row_obj));
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(Value::Array(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_value_table() {
        let html = r#"
            <table>
                <tr><td>Name</td><td>John Doe</td></tr>
                <tr><td>Age</td><td>30</td></tr>
                <tr><td>City</td><td>New York</td></tr>
            </table>
        "#;

        let map = parse_html(html).unwrap();

        assert_eq!(
            map.get("Name").unwrap(),
            &Value::String("John Doe".to_string())
        );
        assert_eq!(map.get("Age").unwrap(), &Value::String("30".to_string()));
        assert_eq!(
            map.get("City").unwrap(),
            &Value::String("New York".to_string())
        );
    }

    #[test]
    fn test_parse_row_table() {
        let html = r#"
            <table>
                <tr><th>Name</th><th>Age</th><th>City</th></tr>
                <tr><td>John</td><td>30</td><td>NYC</td></tr>
                <tr><td>Jane</td><td>25</td><td>LA</td></tr>
            </table>
        "#;

        if let Some(Value::Array(rows)) = parse_html(html) {
            assert_eq!(rows.len(), 2);
            assert_eq!(
                rows[0].get("Name").unwrap(),
                &Value::String("John".to_string())
            );
            assert_eq!(
                rows[0].get("Age").unwrap(),
                &Value::String("30".to_string())
            );
            assert_eq!(
                rows[1].get("Name").unwrap(),
                &Value::String("Jane".to_string())
            );
        } else {
            panic!("Expected Value::Array");
        }
    }

    #[test]
    fn test_parse_list() {
        let html = r#"
            <ul>
                <li>First item</li>
                <li>Second item</li>
                <li>Third item</li>
            </ul>
        "#;

        if let Some(Value::Array(items)) = parse_html(html) {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], "First item");
            assert_eq!(items[1], "Second item");
            assert_eq!(items[2], "Third item");
        } else {
            panic!("Expected Value::Array");
        }
    }

    #[test]
    fn test_parse_ordered_list() {
        let html = r#"
            <ol>
                <li>Step one</li>
                <li>Step two</li>
            </ol>
        "#;

        if let Some(Value::Array(items)) = parse_html(html) {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], "Step one");
            assert_eq!(items[1], "Step two");
        } else {
            panic!("Expected Value::Array");
        }
    }

    #[test]
    fn test_no_structured_content() {
        let html = r#"<p>Just some regular paragraph text with no tables or lists.</p>"#;

        let result = parse_html(html);
        assert!(result.is_none());
    }

    #[test]
    fn test_malformed_html() {
        let html = r#"<table><tr><td>broken"#;
        let result = parse_html(html);
        assert!(result.is_none());
    }

    #[test]
    fn test_malformed_html_early_eof_in_table() {
        let html = r#"<table><tr><td>Name</td><td>John</td></tr><tr><td>Age"#;

        // Should succeed with just the one complete row (single row key-value is now valid)
        if let Some(Value::Object(map)) = parse_html(html) {
            assert_eq!(map.len(), 1);
            assert_eq!(map.get("Name").unwrap(), &Value::String("John".to_string()));
            assert!(!map.contains_key("Age")); // Incomplete row ignored
        } else {
            panic!("Expected KeyValueTable with 1 complete row");
        }
    }

    #[test]
    fn test_single_row_table() {
        let html = r#"<table><tr><td>Status</td><td>Active</td></tr></table>"#;

        if let Some(Value::Object(map)) = parse_html(html) {
            assert_eq!(map.len(), 1);
            assert_eq!(
                map.get("Status").unwrap(),
                &Value::String("Active".to_string())
            );
        } else {
            panic!("Expected single-row key-value table");
        }
    }

    #[test]
    fn test_malformed_html_early_eof_in_list() {
        let html = r#"<ul><li>Item 1</li><li>Item 2"#;

        // Should extract only the complete item
        if let Some(Value::Array(items)) = parse_html(html) {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0], "Item 1");
        } else {
            panic!("Expected list with one complete item");
        }
    }

    #[test]
    fn test_html_table_not_first_element() {
        let html = r#"<p>Some introductory text</p><table><tr><td>Name</td><td>John</td></tr><tr><td>Age</td><td>30</td></tr></table>"#;

        let result = parse_html(html);
        assert!(result.is_some());

        if let Some(Value::Object(map)) = parse_html(html) {
            assert_eq!(map.get("Name").unwrap(), &Value::String("John".to_string()));
            assert_eq!(map.get("Age").unwrap(), &Value::String("30".to_string()));
        } else {
            panic!("Expected KeyValueTable");
        }
    }

    #[test]
    fn test_html_list_not_first_element() {
        let html = r#"<p>Some text</p><ul><li>First</li><li>Second</li></ul>"#;

        let result = parse_html(html);
        assert!(result.is_some());

        if let Some(Value::Array(items)) = parse_html(html) {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], "First");
            assert_eq!(items[1], "Second");
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn test_nested_formatting_in_table_cells() {
        let html = r#"<table><tr><td>Name</td><td>John <i>middle</i> Doe</td></tr><tr><td>Status</td><td><strong>Active</strong></td></tr></table>"#;

        if let Some(Value::Object(map)) = parse_html(html) {
            assert_eq!(
                map.get("Name").unwrap(),
                &Value::String("John middle Doe".to_string())
            );
            assert_eq!(
                map.get("Status").unwrap(),
                &Value::String("Active".to_string())
            );
        } else {
            panic!("Expected KeyValueTable with flattened text");
        }
    }

    #[test]
    fn test_nested_formatting_in_list_items() {
        let html =
            r#"<ul><li>Item <b>bold</b> text</li><li>Link: <a href="url">click here</a></li></ul>"#;

        if let Some(Value::Array(items)) = parse_html(html) {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], "Item bold text");
            assert_eq!(items[1], "Link: click here");
        } else {
            panic!("Expected List with flattened text");
        }
    }

    #[test]
    fn test_table_with_colspan_fails() {
        let html = r#"<table><tr><td>Name</td><td colspan="2">John Doe</td></tr></table>"#;

        let result = parse_html(html);
        assert!(result.is_none(), "Should fail on colspan attribute");
    }

    #[test]
    fn test_table_with_rowspan_fails() {
        let html = r#"<table><tr><td rowspan="2">Name</td><td>John</td></tr><tr><td>Age</td></tr></table>"#;

        let result = parse_html(html);
        assert!(result.is_none(), "Should fail on rowspan attribute");
    }

    #[test]
    fn test_single_level_parsing_ignores_nested_tables() {
        let html = r#"<table><tr><td>Outer</td><td>Cell</td></tr></table><p>Between</p><table><tr><td>Inner</td><td>Table</td></tr></table>"#;

        let result = parse_html(html);
        assert!(result.is_some());

        // Should only parse the first table
        if let Some(Value::Object(map)) = parse_html(html) {
            assert_eq!(
                map.get("Outer").unwrap(),
                &Value::String("Cell".to_string())
            );
            assert!(!map.contains_key("Inner")); // Second table ignored
        } else {
            panic!("Expected first table only");
        }
    }
}
