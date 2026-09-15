Repair the personal analytics narrative object for this operation.

operationId: {{ operation_id }}
Invalid report file: {{ invalid_report_path }}

Validation errors:

{{ validation_errors }}

Target JSON Schema:

{{ report_schema }}

Read the invalid report file, make only the structural repairs required to satisfy the schema above, and return only the repaired JSON object.
