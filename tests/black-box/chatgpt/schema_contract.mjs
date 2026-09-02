// Validate the JSON Schema keywords used by the public MCP catalog, without
// changing requests or filling in missing fields in the application response.
export function assertPublicSchema(value, schema, path = "$result") {
  if (schema === true) return;
  if (schema === false) throw new Error(`${path}: value is forbidden`);
  if (schema.anyOf) {
    const errors = [];
    for (const alternative of schema.anyOf) {
      try { assertPublicSchema(value, alternative, path); return; }
      catch (error) { errors.push(error.message); }
    }
    throw new Error(`${path}: no schema alternative matched: ${errors.join("; ")}`);
  }
  const types = schema.type == null ? [] : [schema.type].flat();
  const actual = value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
  if (types.length && !types.some((type) => type === actual || (type === "integer" && Number.isInteger(value)))) {
    throw new Error(`${path}: expected ${types.join("|")}, received ${actual}`);
  }
  if (schema.enum && !schema.enum.some((candidate) => JSON.stringify(candidate) === JSON.stringify(value))) {
    throw new Error(`${path}: value is outside the published enum`);
  }
  if (Object.hasOwn(schema, "const") && JSON.stringify(schema.const) !== JSON.stringify(value)) {
    throw new Error(`${path}: value differs from the published constant`);
  }
  if (typeof value === "number") {
    if (schema.minimum != null && value < schema.minimum) throw new Error(`${path}: below minimum`);
    if (schema.maximum != null && value > schema.maximum) throw new Error(`${path}: above maximum`);
  }
  if (typeof value === "string") {
    const length = [...value].length;
    if (schema.minLength != null && length < schema.minLength) throw new Error(`${path}: string too short`);
    if (schema.maxLength != null && length > schema.maxLength) throw new Error(`${path}: string too long`);
  }
  if (Array.isArray(value)) {
    if (schema.maxItems != null && value.length > schema.maxItems) throw new Error(`${path}: too many items`);
    if (schema.items) value.forEach((item, index) => assertPublicSchema(item, schema.items, `${path}[${index}]`));
  } else if (value != null && typeof value === "object") {
    for (const key of schema.required ?? []) {
      if (!Object.hasOwn(value, key)) throw new Error(`${path}.${key}: required field missing`);
    }
    for (const [key, item] of Object.entries(value)) {
      const field = schema.properties?.[key] ?? schema.additionalProperties;
      if (field === false) throw new Error(`${path}.${key}: field is not in the published schema`);
      if (field && typeof field === "object") assertPublicSchema(item, field, `${path}.${key}`);
    }
  }
}
