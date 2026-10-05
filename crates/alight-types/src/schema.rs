/// Schema marker for lossless canonical unsigned 64-bit decimal JSON strings.
pub struct DecimalU64;
impl schemars::JsonSchema for DecimalU64 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DecimalU64".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let max = "18446744073709551615";
        let mut branches = vec!["0".to_owned(), "[1-9][0-9]{0,18}".into()];
        for (i, b) in max.bytes().enumerate() {
            let digit = b - b'0';
            let minimum = if i == 0 { 1 } else { 0 };
            if digit > minimum {
                branches.push(format!(
                    "{}[{}-{}][0-9]{{{}}}",
                    &max[..i],
                    minimum,
                    digit - 1,
                    max.len() - i - 1
                ));
            }
        }
        branches.push(max.into());
        let pattern = format!("^({})$", branches.join("|"));
        schemars::json_schema!({"type":"string","format":"uint64-decimal","pattern":pattern})
    }
}
