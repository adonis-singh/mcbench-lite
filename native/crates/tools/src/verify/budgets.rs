/// The verifier's budgets (`BUDGETS`), read once at start from
/// `MCBENCH_VERIFY_MAX_*` (`BUDGET_ENV`) as `readBudgets` reads them.
/// The tool-contract hash covers them, in this order: serialized, they
/// are `BUDGETS`'s keys in its order (`mcbench_json::to_string` writes
/// `JSON.stringify(BUDGETS)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budgets {
    /// `OPERATIONS`.
    pub operations: u64,
    /// `SIMULATION_TICKS`.
    pub simulation_ticks: u64,
    /// `WALL_MS`.
    pub wall_ms: u64,
    /// `COLUMNS`.
    pub columns: u64,
    /// `TESTS`: experiments per call (the `test_build` description
    /// quotes it).
    pub tests: u64,
    /// `REQUEST_BYTES`.
    pub request_bytes: u64,
    /// `REPORT_BYTES`.
    pub report_bytes: u64,
}

impl Budgets {
    /// The budgets when no `MCBENCH_VERIFY_MAX_*` is set (`readBudgets`'
    /// `defaults`).
    pub const DEFAULT: Budgets = Budgets {
        operations: 1_000_000,
        simulation_ticks: 72_000,
        wall_ms: 900_000,
        columns: 16_384,
        tests: 8,
        request_bytes: 8 * 1024 * 1024,
        report_bytes: 128 * 1024 * 1024,
    };

    /// Each budget's environment variable, in `BUDGETS`'s order
    /// (`BUDGET_ENV`).
    pub const ENV: [&'static str; 7] = [
        "MCBENCH_VERIFY_MAX_OPERATIONS",
        "MCBENCH_VERIFY_MAX_SIMULATION_TICKS",
        "MCBENCH_VERIFY_MAX_WALL_MS",
        "MCBENCH_VERIFY_MAX_COLUMNS",
        "MCBENCH_VERIFY_MAX_TESTS",
        "MCBENCH_VERIFY_MAX_REQUEST_BYTES",
        "MCBENCH_VERIFY_MAX_REPORT_BYTES",
    ];

    /// The budgets of this process's environment.
    pub fn from_env() -> Result<Budgets, String> {
        Budgets::from_vars(|name| std::env::var_os(name))
    }

    /// The budgets of the environment `var` reads (`readBudgets(env)`):
    /// each variable that is set is read as JavaScript's `Number` reads a
    /// string and must be an integer within 1..2147483647 (1..2^53-1 for
    /// the report's bytes), or this is `readBudgets`' error for the first
    /// one in `BUDGETS`'s order that is not.
    pub fn from_vars(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<Budgets, String> {
        let defaults = Budgets::DEFAULT;
        let mut values = [
            defaults.operations,
            defaults.simulation_ticks,
            defaults.wall_ms,
            defaults.columns,
            defaults.tests,
            defaults.request_bytes,
            defaults.report_bytes,
        ];
        for (at, name) in Budgets::ENV.into_iter().enumerate() {
            let Some(text) = var(name) else { continue };
            let maximum: u64 = if name == "MCBENCH_VERIFY_MAX_REPORT_BYTES" {
                (1 << 53) - 1
            } else {
                2_147_483_647
            };
            // A value that is not Unicode is NaN to `Number` once Node
            // has decoded it.
            let value = text.to_str().map_or(f64::NAN, string_to_number);
            if !(value.fract() == 0.0 && value >= 1.0 && value <= maximum as f64) {
                return Err(format!("{name} must be an integer within 1..{maximum}"));
            }
            values[at] = value as u64;
        }
        let [
            operations,
            simulation_ticks,
            wall_ms,
            columns,
            tests,
            request_bytes,
            report_bytes,
        ] = values;
        Ok(Budgets {
            operations,
            simulation_ticks,
            wall_ms,
            columns,
            tests,
            request_bytes,
            report_bytes,
        })
    }
}

/// JavaScript's `Number(text)` (ECMA-262 StringToNumber) as far as a
/// budget needs it: exact for every integer up to 2^53, NaN where
/// JavaScript gives NaN. Hexadecimal, octal and binary literals above
/// 2^53 come out approximately (any of them is refused as a budget).
fn string_to_number(text: &str) -> f64 {
    let text = super::js::trim(text);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |x, c| {
                x * f64::from(radix) + f64::from(c.to_digit(radix).expect("a digit"))
            });
        }
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // StrDecimalLiteral is Rust's float grammar without its inf and NaN
    // words, which the characters allowed here exclude.
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

/// The budgets an experiment is checked against and carries in its request
/// (`request.budgets`): `BUDGETS`, or the object a benchmark grader gives
/// its verifier (`verifier.budgets = {...}`), its keys in its order. A key
/// it lacks is a budget that never refuses (`requested > undefined` is
/// false), as today.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits(pub Vec<(String, f64)>);

/// `BUDGETS`' keys, in `BUDGET_ENV`'s order.
const RESOURCES: [&str; 7] = [
    "operations",
    "simulationTicks",
    "wallMs",
    "columns",
    "tests",
    "requestBytes",
    "reportBytes",
];

impl Limits {
    /// The limit named `resource` (`budgets[resource]`).
    pub fn get(&self, resource: &str) -> Option<f64> {
        self.0
            .iter()
            .find(|(name, _)| name == resource)
            .map(|(_, value)| *value)
    }

    /// The limits as the JSON object `{ ...budgets }` writes.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.0
                .iter()
                .map(|(name, value)| (name.clone(), super::js::number(*value)))
                .collect(),
        )
    }

    /// The limits of a JSON object whose values are numbers (a grader's
    /// `budgets`).
    pub fn from_json(value: &serde_json::Value) -> Result<Limits, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "budgets must be an object of numbers".to_string())?;
        object
            .iter()
            .map(|(name, value)| {
                value
                    .as_f64()
                    .map(|number| (name.clone(), number))
                    .ok_or_else(|| format!("budgets.{name} must be a number"))
            })
            .collect::<Result<_, _>>()
            .map(Limits)
    }

    /// The setting a resource's limit comes from (`BUDGET_ENV[resource]`).
    pub fn setting(resource: &str) -> Option<&'static str> {
        let at = RESOURCES.iter().position(|name| *name == resource)?;
        Some(Budgets::ENV[at])
    }
}

impl From<Budgets> for Limits {
    fn from(budgets: Budgets) -> Limits {
        let values = [
            budgets.operations,
            budgets.simulation_ticks,
            budgets.wall_ms,
            budgets.columns,
            budgets.tests,
            budgets.request_bytes,
            budgets.report_bytes,
        ];
        Limits(
            RESOURCES
                .iter()
                .zip(values)
                .map(|(name, value)| (name.to_string(), value as f64))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn budgets(vars: &[(&str, &str)]) -> Result<Budgets, String> {
        let vars: HashMap<String, std::ffi::OsString> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.into()))
            .collect();
        Budgets::from_vars(|name| vars.get(name).cloned())
    }

    #[test]
    fn budgets_are_read_as_read_budgets_reads_them() {
        assert_eq!(budgets(&[]), Ok(Budgets::DEFAULT));
        let read = budgets(&[
            ("MCBENCH_VERIFY_MAX_TESTS", " 12\u{a0}"),
            ("MCBENCH_VERIFY_MAX_COLUMNS", "0x10"),
            ("MCBENCH_VERIFY_MAX_WALL_MS", "1e3"),
            ("MCBENCH_VERIFY_MAX_OPERATIONS", "+7.0"),
            ("MCBENCH_VERIFY_MAX_REPORT_BYTES", "9007199254740991"),
        ])
        .unwrap();
        assert_eq!(
            (
                read.tests,
                read.columns,
                read.wall_ms,
                read.operations,
                read.report_bytes
            ),
            (12, 16, 1000, 7, 9007199254740991)
        );
        for bad in [
            "",
            "0",
            "-1",
            "1.5",
            "2147483648",
            "abc",
            "inf",
            "Infinity",
            "0x",
            "\u{85}5",
            "1_0",
        ] {
            assert_eq!(
                budgets(&[("MCBENCH_VERIFY_MAX_TESTS", bad)]),
                Err("MCBENCH_VERIFY_MAX_TESTS must be an integer within 1..2147483647".to_owned()),
                "{bad:?}"
            );
        }
        assert_eq!(
            budgets(&[
                ("MCBENCH_VERIFY_MAX_REPORT_BYTES", "9007199254740992"),
                ("MCBENCH_VERIFY_MAX_TESTS", "0"),
            ]),
            Err("MCBENCH_VERIFY_MAX_TESTS must be an integer within 1..2147483647".to_owned())
        );
        assert_eq!(
            budgets(&[("MCBENCH_VERIFY_MAX_REPORT_BYTES", "9007199254740992")]),
            Err(
                "MCBENCH_VERIFY_MAX_REPORT_BYTES must be an integer within 1..9007199254740991"
                    .to_owned()
            )
        );
    }
}
