// Generated from alight-types via alight-api::openapi, never handwritten wire shapes.
import {readFileSync, writeFileSync} from 'node:fs';
const schemas = JSON.parse(readFileSync(new URL('./openapi.json', import.meta.url))).components.schemas;
function type(s) {
  if (s === true) return 'unknown';
  if (s === false) return 'never';
  if (s.$ref) return s.$ref.split('/').at(-1);
  if (s.const !== undefined) return JSON.stringify(s.const);
  if (s.enum) return s.enum.map(x => JSON.stringify(x)).join(' | ');
  for (const key of ['oneOf','anyOf','allOf']) if (s[key]) return '(' + s[key].map(type).join(key==='allOf'?' & ':' | ') + ')';
  if (Array.isArray(s.type)) return '('+s.type.map(t => type({...s,type:t})).join(' | ')+')';
  switch(s.type) {
    case 'null': return 'null';
    case 'string': return 'string';
    case 'boolean': return 'boolean';
    case 'integer': case 'number': return 'number';
    case 'array': return s.prefixItems ? '['+s.prefixItems.map(type).join(', ')+']' : `Array<${type(s.items ?? true)}>`;
    case 'object': {
      const properties=Object.entries(s.properties??{}).map(([k,v]) => `${JSON.stringify(k)}${s.required?.includes(k)?'':'?'}: ${type(v)}`);
      if (s.additionalProperties && typeof s.additionalProperties==='object') properties.push(`[key: string]: ${type(s.additionalProperties)}`);
      return properties.length ? '{ '+properties.join('; ')+' }' : 'Record<string, unknown>';
    }
    default: return 'unknown';
  }
}
const types='// GENERATED. Run cargo run -p alight-api --example openapi > sdk/ts/openapi.json then npm run generate.\n'+Object.entries(schemas).sort(([a],[b])=>a.localeCompare(b)).map(([k,v])=>`export type ${k} = ${k==='DecimalU64'?'string & { readonly __decimalU64: unique symbol }':type(v)};`).join('\n')+'\n';
const runtime='// GENERATED from the same serde schemas as types.ts.\nimport type { Schema } from "./validation.js";\nexport const schemas: Record<string, Schema> = '+JSON.stringify(schemas,null,2)+';\n';
for (const [path,content] of [['src/types.ts',types],['src/schemas.ts',runtime]]) {
  const url=new URL(path,import.meta.url);
  if (process.argv.includes('--check')) { if(readFileSync(url,'utf8')!==content) throw new Error('Generated SDK contracts differ'); }
  else writeFileSync(url,content);
}
