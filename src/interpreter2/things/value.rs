#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PropertyValue {
    Value {
        value: ftd::interpreter2::Value,
        line_number: usize,
    },
    Reference {
        name: String,
        kind: ftd::interpreter2::KindData,
        line_number: usize,
    },
}

impl PropertyValue {
    pub(crate) fn from_string(
        value: &str,
        doc: &ftd::interpreter2::TDoc,
        expected_kind: Option<&ftd::interpreter2::KindData>,
        line_number: usize,
    ) -> ftd::interpreter2::Result<ftd::interpreter2::PropertyValue> {
        let value = ftd::ast::VariableValue::String {
            value: value.to_string(),
            line_number,
        };

        PropertyValue::from_ast_value_with_kind(value, doc, expected_kind)
    }

    pub(crate) fn from_ast_value_with_kind(
        value: ftd::ast::VariableValue,
        doc: &ftd::interpreter2::TDoc,
        expected_kind: Option<&ftd::interpreter2::KindData>,
    ) -> ftd::interpreter2::Result<ftd::interpreter2::PropertyValue> {
        if let Ok(reference) = PropertyValue::reference_from_ast_value(&value, doc, expected_kind) {
            return Ok(reference);
        }
        let expected_kind = expected_kind.ok_or(ftd::interpreter2::Error::ParseError {
            message: "Need expected kind".to_string(),
            doc_id: doc.name.to_string(),
            line_number: value.line_number(),
        })?;
        Ok(match &expected_kind.kind {
            ftd::interpreter2::Kind::String => PropertyValue::Value {
                value: Value::String {
                    text: value.string(doc.name)?,
                },
                line_number: value.line_number(),
            },
            ftd::interpreter2::Kind::Integer => PropertyValue::Value {
                value: Value::Integer {
                    value: value.string(doc.name)?.parse()?,
                },
                line_number: value.line_number(),
            },
            ftd::interpreter2::Kind::Decimal => PropertyValue::Value {
                value: Value::Decimal {
                    value: value.string(doc.name)?.parse()?,
                },
                line_number: value.line_number(),
            },
            ftd::interpreter2::Kind::Boolean => PropertyValue::Value {
                value: Value::Boolean {
                    value: value.string(doc.name)?.parse()?,
                },
                line_number: value.line_number(),
            },
            ftd::interpreter2::Kind::List { kind } if value.is_list() => {
                let line_number = value.line_number();
                let value_list = value.into_list(doc.name)?;
                let mut values = vec![];
                for (key, value) in value_list {
                    if !ftd::interpreter2::utils::kind_eq(
                        key.as_str(),
                        kind,
                        doc,
                        value.line_number(),
                    )? {
                        return ftd::interpreter2::utils::e2(
                            format!("Expected list of `{:?}`, found: `{}`", kind, key),
                            doc.name,
                            value.line_number(),
                        );
                    }
                    values.push(PropertyValue::from_ast_value_with_kind(
                        value,
                        doc,
                        Some(&ftd::interpreter2::KindData {
                            kind: kind.as_ref().clone(),
                            caption: expected_kind.caption,
                            body: expected_kind.body,
                        }),
                    )?);
                }
                PropertyValue::Value {
                    value: ftd::interpreter2::Value::List {
                        data: values,
                        kind: expected_kind.clone(),
                    },
                    line_number,
                }
            }
            ftd::interpreter2::Kind::Record { name } if value.is_record() => {
                let record = doc.get_record(value.line_number(), name)?;
                let (_, caption, headers, body, line_number) = value.get_record(doc.name)?;
                // TODO: Check if the record name and the value kind are same
                // if !doc.eq(name, rec_name) {
                //     return ftd::interpreter2::utils::e2(
                //         format!("Expected record of `{}`, found: `{}`", name, rec_name),
                //         doc.name,
                //         value.line_number(),
                //     );
                // }
                let mut result_field: ftd::Map<PropertyValue> = Default::default();
                for field in record.fields {
                    if field.is_caption() && caption.is_some() {
                        let caption = caption.as_ref().as_ref().unwrap().clone();
                        result_field.insert(
                            field.name.to_string(),
                            PropertyValue::from_ast_value_with_kind(
                                caption,
                                doc,
                                Some(&field.kind),
                            )?,
                        );
                        continue;
                    }
                    if field.is_body() && body.is_some() {
                        let body = body.as_ref().unwrap();
                        result_field.insert(
                            field.name.to_string(),
                            PropertyValue::from_ast_value_with_kind(
                                ftd::ast::VariableValue::String {
                                    value: body.value.to_string(),
                                    line_number: body.line_number,
                                },
                                doc,
                                Some(&field.kind),
                            )?,
                        );
                        continue;
                    }
                    let headers = headers.get_by_key(field.name.as_str());
                    if headers.is_empty() && field.kind.is_optional() {
                        result_field.insert(
                            field.name.to_string(),
                            PropertyValue::Value {
                                value: ftd::interpreter2::Value::Optional {
                                    data: Box::new(None),
                                    kind: expected_kind.to_owned(),
                                },
                                line_number,
                            },
                        );
                        continue;
                    }
                    if field.kind.is_list() {
                        let mut header_list = vec![];
                        for header in headers {
                            header_list.extend(match &header.value {
                                ftd::ast::VariableValue::List { value, .. } => value.to_owned(),
                                t => vec![(header.key.to_string(), t.to_owned())],
                            });
                        }
                        result_field.insert(
                            field.name.to_string(),
                            PropertyValue::from_ast_value_with_kind(
                                ftd::ast::VariableValue::List {
                                    value: header_list,
                                    line_number: value.line_number(),
                                },
                                doc,
                                Some(&field.kind),
                            )?,
                        );
                        continue;
                    }
                    if headers.len() != 1 {
                        return ftd::interpreter2::utils::e2(
                            format!(
                                "Expected `{}` of type `{:?}`, found: `{:?}`",
                                field.name, field.kind, headers
                            ),
                            doc.name,
                            value.line_number(),
                        );
                    }
                    let first_header = headers.first().unwrap();
                    result_field.insert(
                        field.name.to_string(),
                        PropertyValue::from_ast_value_with_kind(
                            first_header.value.clone(),
                            doc,
                            Some(&field.kind),
                        )?,
                    );
                }
                PropertyValue::Value {
                    value: ftd::interpreter2::Value::Record {
                        name: name.to_string(),
                        fields: result_field,
                    },
                    line_number,
                }
            }
            _ => {
                unimplemented!()
            }
        })
    }

    fn reference_from_ast_value(
        value: &ftd::ast::VariableValue,
        doc: &ftd::interpreter2::TDoc,
        expected_kind: Option<&ftd::interpreter2::KindData>,
    ) -> ftd::interpreter2::Result<ftd::interpreter2::PropertyValue> {
        match value.string(doc.name) {
            Ok(name) if name.starts_with(ftd::interpreter2::utils::REFERENCE) => {
                let reference = name
                    .trim_start_matches(ftd::interpreter2::utils::REFERENCE)
                    .to_string();

                let found_kind = doc.get_kind(reference.as_str(), value.line_number())?;

                match expected_kind {
                    Some(ekind) if !ekind.kind.eq(&found_kind.kind) => {
                        return ftd::interpreter2::utils::e2(
                            format!("Expected kind `{:?}`, found: `{:?}`", ekind, found_kind)
                                .as_str(),
                            doc.name,
                            value.line_number(),
                        )
                    }
                    _ => {}
                }

                Ok(PropertyValue::Reference {
                    name: reference,
                    kind: found_kind,
                    line_number: value.line_number(),
                })
            }
            _ => ftd::interpreter2::utils::e2(
                format!("Expected reference, found: `{:?}`", value),
                doc.name,
                value.line_number(),
            ),
        }
    }

    pub(crate) fn kind(&self) -> ftd::interpreter2::Kind {
        match self {
            PropertyValue::Value { value, .. } => value.kind(),
            PropertyValue::Reference { kind, .. } => kind.kind.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum Value {
    String {
        text: String,
    },
    Integer {
        value: i64,
    },
    Decimal {
        value: f64,
    },
    Boolean {
        value: bool,
    },
    Object {
        values: ftd::Map<PropertyValue>,
    },
    Record {
        name: String,
        fields: ftd::Map<PropertyValue>,
    },
    List {
        data: Vec<PropertyValue>,
        kind: ftd::interpreter2::KindData,
    },
    Optional {
        data: Box<Option<Value>>,
        kind: ftd::interpreter2::KindData,
    },
    UI {
        name: String,
        // component: ftd::interpreter::Component,
    },
}

impl Value {
    fn kind(&self) -> ftd::interpreter2::Kind {
        match self {
            Value::String { .. } => ftd::interpreter2::Kind::String,
            Value::Integer { .. } => ftd::interpreter2::Kind::Integer,
            Value::Decimal { .. } => ftd::interpreter2::Kind::Decimal,
            Value::Boolean { .. } => ftd::interpreter2::Kind::Boolean,
            Value::Object { .. } => ftd::interpreter2::Kind::Object,
            Value::Record { name, .. } => ftd::interpreter2::Kind::Record {
                name: name.to_string(),
            },
            Value::List { kind, .. } => ftd::interpreter2::Kind::List {
                kind: Box::new(kind.kind.clone()),
            },
            Value::Optional { kind, .. } => ftd::interpreter2::Kind::Optional {
                kind: Box::new(kind.kind.clone()),
            },
            Value::UI { .. } => ftd::interpreter2::Kind::UI,
        }
    }
}
