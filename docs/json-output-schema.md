# CLI JSON Output Schema

All project-oriented `--format json` command outputs use a versioned top-level
envelope. Consumers should branch on `schema_version` before relying on command
payload fields.

Current schema version: `1`.

## Envelope

```json
{
  "schema_version": 1,
  "summary": {},
  "diagnostics": {
    "diagnostics": []
  }
}
```

`schema_version` is stable across:

- `typepython check --format json`
- `typepython build --format json`
- `typepython watch --format json`
- `typepython verify --format json`
- `typepython compat --format json`
- `typepython api-diff --format json`
- `typepython type-health --format json`
- `typepython migrate --format json`
- `typepython adapter validate --format json`

Commands may add command-specific top-level fields in schema version `1`
(`portability`, `pep561`, `api_diff`, `type_health`, `report`, and similar), but
they must not remove or change the meaning of `schema_version`, `summary`, or
`diagnostics` without bumping the schema version.

## Diagnostics

The `diagnostics` value is a `DiagnosticReport`:

```json
{
  "diagnostics": [
    {
      "code": "TPY4001",
      "severity": "error",
      "message": "value is not assignable",
      "notes": [],
      "suggestions": [],
      "span": {
        "path": "src/app/main.tpy",
        "line": 1,
        "column": 1,
        "end_line": 1,
        "end_column": 5
      }
    }
  ]
}
```

Diagnostic fields:

| Field | Stability |
| ----- | --------- |
| `code` | Stable diagnostic identity within the Beta line |
| `severity` | One of `error`, `warning`, `note` |
| `message` | Human-readable; wording may improve without a schema bump |
| `notes` | Human-readable supporting context |
| `suggestions` | Machine-readable replacement suggestions |
| `span` | Optional 1-based source range |

Suggestion fields are `message`, `span`, `replacement`, and `applicability`
(`machineApplicable` or `maybeIncorrect`).

## Versioning Rules

Schema version `1` allows additive fields. A future schema version is required
for removing fields, changing field types, renaming fields, changing diagnostic
severity spelling, or changing 1-based span coordinates.
