/** The JSON Schema 2020-12 subset emitted by pinned schemars for these contracts. */
export type Schema = boolean | {
  readonly [key: string]: unknown;
  readonly $ref?: string; readonly const?: unknown; readonly enum?: readonly unknown[];
  readonly type?: string | readonly string[];
  readonly anyOf?: readonly Schema[]; readonly oneOf?: readonly Schema[]; readonly allOf?: readonly Schema[];
  readonly properties?: Readonly<Record<string,Schema>>; readonly required?: readonly string[];
  readonly additionalProperties?: Schema;
  readonly items?: Schema; readonly prefixItems?: readonly Schema[];
  readonly minItems?: number; readonly maxItems?: number;
  readonly minimum?: number; readonly maximum?: number;
  readonly exclusiveMinimum?: number; readonly exclusiveMaximum?: number;
  readonly minLength?: number; readonly maxLength?: number; readonly pattern?: string;
};
export function matches(schema: Schema, value: unknown, schemas: Readonly<Record<string,Schema>>): boolean {
  if (typeof schema === 'boolean') return schema;
  if (schema.$ref) { const target=schemas[schema.$ref.split('/').at(-1) ?? '']; return target!==undefined && matches(target,value,schemas); }
  if ('const' in schema && schema.const!==value) return false;
  if (schema.enum && !schema.enum.includes(value)) return false;
  if (schema.anyOf && !schema.anyOf.some(s=>matches(s,value,schemas))) return false;
  if (schema.oneOf && schema.oneOf.filter(s=>matches(s,value,schemas)).length!==1) return false;
  if (schema.allOf && !schema.allOf.every(s=>matches(s,value,schemas))) return false;
  if (Array.isArray(schema.type)) return schema.type.some(t=>matches({...schema,type:t},value,schemas));
  switch(schema.type) {
    case 'null': return value===null;
    case 'boolean': return typeof value==='boolean';
    case 'number': case 'integer':
      return typeof value==='number' && Number.isFinite(value) && (schema.type!=='integer'||Number.isInteger(value)) &&
        (schema.minimum===undefined||value>=schema.minimum) && (schema.maximum===undefined||value<=schema.maximum) &&
        (schema.exclusiveMinimum===undefined||value>schema.exclusiveMinimum) && (schema.exclusiveMaximum===undefined||value<schema.exclusiveMaximum);
    case 'string': return typeof value==='string' && (schema.minLength===undefined||value.length>=schema.minLength) && (schema.maxLength===undefined||value.length<=schema.maxLength) && (!schema.pattern||new RegExp(schema.pattern).test(value));
    case 'array': return Array.isArray(value) && (schema.minItems===undefined||value.length>=schema.minItems) && (schema.maxItems===undefined||value.length<=schema.maxItems) && value.every((v,i)=>matches(schema.prefixItems?.[i]??schema.items??true,v,schemas));
    case 'object': {
      if (typeof value!=='object'||value===null||Array.isArray(value)) return false;
      const object=value as Record<string,unknown>;
      return (schema.required??[]).every(k=>Object.hasOwn(object,k)) && Object.entries(object).every(([k,v])=>matches(schema.properties?.[k]??schema.additionalProperties??true,v,schemas));
    }
    default: return true;
  }
}
