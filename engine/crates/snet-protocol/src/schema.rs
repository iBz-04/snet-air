use snet_cloud::OutboundText;
use snet_pseudonym::{OutboundJson, PseudonymError, Pseudonymiser};

/// A cloud reply schema built from static strings and citation numbers
///: `step` an enum of `steps`, `text` a string, `citations`
/// an array whose items are an enum of `issued` (or strings when none is),
/// then `extra` properties, all required and nothing else.
pub fn step_schema(
    pseudonymiser: &Pseudonymiser<'_>,
    steps: &[&'static str],
    issued: &[u32],
    extra: Vec<(&'static str, OutboundJson)>,
) -> Result<OutboundText, PseudonymError> {
    let string = || ("type", OutboundJson::Static("string"));
    let items = if issued.is_empty() {
        OutboundJson::Object(vec![string()])
    } else {
        OutboundJson::Object(vec![
            string(),
            (
                "enum",
                OutboundJson::Array(
                    issued
                        .iter()
                        .map(|number| OutboundJson::Numbered("C", *number))
                        .collect(),
                ),
            ),
        ])
    };
    let mut required = vec![
        OutboundJson::Static("step"),
        OutboundJson::Static("text"),
        OutboundJson::Static("citations"),
    ];
    required.extend(extra.iter().map(|(name, _)| OutboundJson::Static(name)));
    let mut properties = vec![
        (
            "step",
            OutboundJson::Object(vec![
                string(),
                (
                    "enum",
                    OutboundJson::Array(
                        steps
                            .iter()
                            .map(|step| OutboundJson::Static(step))
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("text", OutboundJson::Object(vec![string()])),
        (
            "citations",
            OutboundJson::Object(vec![
                ("type", OutboundJson::Static("array")),
                ("items", items),
            ]),
        ),
    ];
    properties.extend(extra);
    pseudonymiser.typed(&OutboundJson::Object(vec![
        ("type", OutboundJson::Static("object")),
        ("properties", OutboundJson::Object(properties)),
        ("required", OutboundJson::Array(required)),
        ("additionalProperties", OutboundJson::Bool(false)),
    ]))
}
