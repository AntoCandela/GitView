/** Parses policy/evidence source without depending on compiler internals or evaluating expressions. */
import { parseSync } from 'rolldown/utils';

/** Parse failures are unavailable evidence; parser diagnostics must not enter public reports. */
export function visitSource(path, source, visit) {
  const parsed = parseSync(path, source, { astType: 'ts', preserveParens: true });
  if (parsed.errors.length) throw new Error('source_parse_failed');
  function walk(node) {
    if (!node || typeof node !== 'object' || typeof node.type !== 'string') return;
    visit(node);
    for (const [key, value] of Object.entries(node)) {
      if (key === 'parent') continue;
      if (Array.isArray(value)) value.forEach(walk);
      else walk(value);
    }
  }
  walk(parsed.program);
}

/** Only decoded literals confer static authority; do not unwrap or evaluate expressions. */
export function staticString(node) {
  if (node?.type === 'Literal' && typeof node.value === 'string') return node.value;
  if (node?.type === 'TemplateLiteral' && node.expressions.length === 0 && node.quasis.length === 1) {
    const value = node.quasis[0].value.cooked;
    return typeof value === 'string' ? value : null;
  }
  return null;
}
