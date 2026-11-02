"""Deterministic, value-free observations. No model or publication operations."""
import hashlib
import json
import os
import re
import unicodedata
from pathlib import Path

PROTOCOL = 'oo.builder.column-observation/v1'

def table_binding(table):
    result = {k: table.get(k) for k in
              ['dataset', 'source_id', 'source_generation', 'target_generation', 'physical', 'ddl_sha256']}
    if 'oracle' in table:
        result['oracle_sha256'] = hashlib.sha256(json.dumps(
            table['oracle'], sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()
    return result


def bind_ddl_columns(source, mappings):
    """Bind source names to connector pgident/v1 names, without guessing."""
    columns, originals = {}, {}
    normalized = {unicodedata.normalize('NFC', name): name for name in source['columns']}
    if len(normalized) != len(source['columns']):
        raise ValueError('column_mapping_identifier_collision')
    for mapping in mappings:
        logical = mapping['logical_column']
        original = logical
        if logical.startswith('q_'):
            encoded = logical[2:]
            if not re.fullmatch(r'(?:[0-9a-f]{2})+', encoded):
                raise ValueError('column_mapping_identifier')
            try:
                original = bytes.fromhex(encoded).decode('utf-8')
            except UnicodeDecodeError:
                raise ValueError('column_mapping_identifier') from None
        original = normalized.get(original, original)
        if (original not in source['columns'] or original in originals
                or logical in columns):
            raise ValueError('column_mapping_coverage')
        originals[original] = logical
        columns[logical] = dict(source['columns'][original])
        if original != logical:
            columns[logical]['source_name'] = original
    if set(originals) != set(source['columns']):
        raise ValueError('column_mapping_coverage')
    return dict(source, columns=columns,
                primary_key=[originals[name] for name in source['primary_key']])

def quote(value):
    return '"' + value.replace('"', '""') + '"'


def literal(value):
    return "'" + value.replace("'", "''") + "'"


def text_shape_sql(physical, column):
    """Value-free JSON shape evidence for a selected new text field on PG 17+."""
    if not isinstance(physical, str) or not re.fullmatch(
            r'"(?:[^"]|"")+"\."(?:[^"]|"")+"', physical):
        raise ValueError('text_shape_physical_binding')
    name = quote(column)
    return (f"select json_build_object('rows',count(*),"
            f"'nulls',count(*) filter(where {name} is null),"
            f"'json_documents',count(*) filter(where {name} is json),"
            f"'json_objects',count(*) filter(where {name} is json object),"
            f"'json_arrays',count(*) filter(where {name} is json array)) from {physical}")


def tokens(sql):
    pattern = r"`(?:``|[^`])*`|'(?:''|\\.|[^'\\])*'|\"(?:\"\"|\\.|[^\"\\])*\"|/\*.*?\*/|--[ \t][^\n]*|\#[^\n]*|[A-Za-z_][A-Za-z_0-9]*|[-+]?\d+(?:\.\d+)?|[^\s]"
    parts = re.findall(pattern, sql, re.S)
    if any(p.startswith('/*!') for p in parts):
        raise ValueError('ddl_executable_comment_unsupported')
    return [p for p in parts if not p.startswith(('/*', '--', '#'))]


def group_end(parts, start):
    depth = 0
    for i in range(start, len(parts)):
        if parts[i] == '(':
            depth += 1
        elif parts[i] == ')':
            depth -= 1
            if depth == 0:
                return i
    raise ValueError('ddl_unbalanced_definition')


def unquote(value):
    if len(value) < 2 or value[0] != "'" or value[-1] != "'":
        raise ValueError('ddl_expected_string')
    escapes = {'0': '\0', 'b': '\b', 'n': '\n', 'r': '\r', 't': '\t', 'Z': '\x1a'}
    return re.sub(r'\\(.)', lambda m: escapes.get(m[1], m[1]),
                  value[1:-1].replace("''", "'"), flags=re.S)


def column_definition(parts):
    name = parts[0][1:-1].replace('``', '`')
    if len(parts) < 2 or not re.fullmatch(r'[A-Za-z]+', parts[1]):
        raise ValueError('ddl_column_type')
    end = 2
    if end < len(parts) and parts[end] == '(':
        end = group_end(parts, end) + 1
    source_type = parts[1].lower() + ''.join(parts[2:end])
    while end < len(parts) and parts[end].upper() in {'UNSIGNED', 'ZEROFILL'}:
        source_type += ' ' + parts[end].lower()
        end += 1
    result = dict(source_type=source_type, nullable=True, default_present=False,
                  default_sql=None, comment='', definition_sql=' '.join(parts))
    while end < len(parts):
        key = parts[end].upper()
        if key == 'NOT' and end + 1 < len(parts) and parts[end + 1].upper() == 'NULL':
            result['nullable'] = False
            end += 2
        elif key in {'NULL', 'AUTO_INCREMENT'}:
            if key == 'AUTO_INCREMENT':
                result['auto_increment'] = True
            end += 1
        elif key in {'DEFAULT', 'COMMENT', 'COLLATE'}:
            if end + 1 >= len(parts):
                raise ValueError('ddl_attribute_value')
            start = end + 1
            finish = start + 1
            if parts[start] == '(':
                finish = group_end(parts, start) + 1
            elif finish < len(parts) and parts[finish] == '(':
                finish = group_end(parts, finish) + 1
            value = ''.join(parts[start:finish])
            if key == 'DEFAULT':
                result.update(default_present=True, default_sql=value)
            elif key == 'COMMENT':
                result['comment'] = unquote(value)
            else:
                result['collation'] = value
            end = finish
        elif key == 'CHARACTER' and end + 2 < len(parts) and parts[end + 1].upper() == 'SET':
            result['charset'] = parts[end + 2]
            end += 3
        elif key == 'ON' and end + 2 < len(parts) and parts[end + 1].upper() == 'UPDATE':
            finish = end + 3
            if finish < len(parts) and parts[finish] == '(':
                finish = group_end(parts, finish) + 1
            result['on_update_sql'] = ''.join(parts[end + 2:finish])
            end = finish
        else:
            # Inline constraints/generated expressions need a deliberate adapter.
            raise ValueError('ddl_unsupported_column_attribute')
    return name, result


def ddl_metadata(directory):
    result = {}
    paths = sorted(Path(directory).glob('*.sql'))
    if not paths:
        raise ValueError('ddl_inventory_empty')
    for path in paths:
        text = path.read_text()
        parts = tokens(text)
        starts = [i for i in range(len(parts) - 1)
                  if parts[i].upper() == 'CREATE' and parts[i + 1].upper() == 'TABLE']
        if len(starts) != 1:
            raise ValueError('ddl_expected_one_create_table')
        prefix = parts[:starts[0]]
        if prefix and not re.fullmatch(r'DROP TABLE IF EXISTS `(?:``|[^`])+` ;', ' '.join(prefix), re.I):
            raise ValueError('ddl_unsupported_preamble')
        i = starts[0] + 2
        if [x.upper() for x in parts[i:i + 3]] == ['IF', 'NOT', 'EXISTS']:
            i += 3
        if i >= len(parts) or not parts[i].startswith('`'):
            raise ValueError('ddl_table_identifier')
        schema, name = None, parts[i][1:-1].replace('``', '`')
        i += 1
        if i < len(parts) and parts[i] == '.':
            if i + 1 >= len(parts) or not parts[i + 1].startswith('`'):
                raise ValueError('ddl_table_identifier')
            schema, name = name, parts[i + 1][1:-1].replace('``', '`')
            i += 2
        if i >= len(parts) or parts[i] != '(' or name in result:
            raise ValueError('ddl_duplicate_or_invalid_table')
        last = group_end(parts, i)
        tail = parts[last + 1:]
        if (';' in tail and tail.index(';') != len(tail) - 1) or any(
                p.upper() in {'SELECT', 'PARTITION', 'ALTER', 'CREATE'} for p in tail):
            raise ValueError('ddl_unsupported_table_options')
        comments = []
        for offset, token in enumerate(tail):
            if token.upper() == 'COMMENT':
                value = offset + 1
                if value < len(tail) and tail[value] == '=':
                    value += 1
                if value >= len(tail):
                    raise ValueError('ddl_table_comment')
                comments.append(unquote(tail[value]))
        if len(comments) > 1:
            raise ValueError('ddl_duplicate_table_comment')
        definitions, start, depth = [], i + 1, 0
        for j in range(i + 1, last + 1):
            if j == last or (parts[j] == ',' and depth == 0):
                definitions.append(parts[start:j])
                start = j + 1
            elif parts[j] == '(':
                depth += 1
            elif parts[j] == ')':
                depth -= 1
        columns, constraints, primary = {}, [], []
        for definition in definitions:
            if not definition:
                raise ValueError('ddl_empty_definition')
            if definition[0].startswith('`'):
                key, column = column_definition(definition)
                if key in columns:
                    raise ValueError('ddl_duplicate_column')
                columns[key] = column
            elif definition[0].upper() in {'PRIMARY', 'UNIQUE', 'KEY', 'INDEX', 'CONSTRAINT', 'FULLTEXT', 'SPATIAL', 'FOREIGN', 'CHECK'}:
                constraints.append(' '.join(definition))
                if definition[0].upper() == 'CONSTRAINT' and any(p.upper() == 'PRIMARY' for p in definition):
                    raise ValueError('ddl_named_primary_unsupported')
                if definition[0].upper() == 'PRIMARY':
                    if primary or len(definition) < 4 or definition[1].upper() != 'KEY':
                        raise ValueError('ddl_primary_key')
                    primary = [p[1:-1].replace('``', '`') for p in definition[2:] if p.startswith('`')]
            else:
                raise ValueError('ddl_unsupported_definition')
        if not columns or not set(primary) <= set(columns):
            raise ValueError('ddl_column_inventory')
        for key in primary:
            columns[key]['nullable'] = False
        result[name] = dict(columns=columns, primary_key=primary, schema=schema,
                            constraints_sql=constraints, table_options_sql=' '.join(parts[last + 1:]),
                            table_comment=comments[0] if comments else '',
                            ddl_sha256=hashlib.sha256(text.encode()).hexdigest())
    return result


def profile_sql(physical, columns):
    """All aggregates share one statement snapshot; never return raw values."""
    objects = []
    for c in columns:
        value = quote(c['physical_name']) + '::text'
        objects.append(
            "jsonb_build_object('name'," + literal(c['name']) +
            ",'non_null',count(" + value + "),'distinct',count(distinct " + value + ")" +
            ",'min_length',min(length(" + value + ")),'max_length',max(length(" + value + "))" +
            ",'blank_rows',count(*) filter(where btrim(" + value + ")='')" +
            ",'digits_rows',count(*) filter(where " + value + " ~ '^[0-9]+$')" +
            ",'leading_zero_rows',count(*) filter(where " + value + " ~ '^0[0-9]+$'))")
    if not objects:
        raise ValueError('observation_empty_columns')
    # PostgreSQL functions have an argument limit; concatenation supports wide tables.
    arrays = ['jsonb_build_array(' + ','.join(objects[i:i + 32]) + ')'
              for i in range(0, len(objects), 32)]
    return ("select jsonb_build_object('rows',count(*),'snapshot',"
            "pg_current_snapshot()::text,'observed_at',statement_timestamp()::text,"
            "'columns'," + ' || '.join(arrays) + ') from ' + physical)


def profiles(result, names):
    rows = result.get('rows')
    if type(rows) is not int or rows < 0:
        raise ValueError('observation_invalid_rows')
    observed = result.get('columns', [])
    if len(observed) != len(names) or {c['name'] for c in observed} != set(names):
        raise ValueError('observation_column_coverage')
    out = {}
    for c in observed:
        counts = [c.get(k) for k in ['non_null', 'distinct', 'blank_rows', 'digits_rows', 'leading_zero_rows']]
        if any(type(n) is not int or not 0 <= n <= rows for n in counts):
            raise ValueError('observation_invalid_counts')
        non_null, distinct, blank, digits, zeros = counts
        lo, hi = c.get('min_length'), c.get('max_length')
        if not (distinct <= non_null and blank <= non_null and zeros <= digits <= non_null
                and blank + digits <= non_null and bool(distinct) == bool(non_null)):
            raise ValueError('observation_inconsistent_counts')
        if non_null:
            if type(lo) is not int or type(hi) is not int or not 0 <= lo <= hi:
                raise ValueError('observation_invalid_lengths')
        elif lo is not None or hi is not None:
            raise ValueError('observation_invalid_lengths')
        out[c['name']] = dict(
            protocol=PROTOCOL, state='empty' if rows == 0 else 'all_null' if non_null == 0 else 'complete',
            method='exact_full_scan', distinct_basis='postgres_text',
            rows=rows, observed_rows=rows, non_null=non_null, null_count=rows - non_null,
            null_frac=(rows - non_null) / rows if rows else None, distinct=distinct,
            min_length=lo, max_length=hi, blank_rows=blank, digits_rows=digits,
            leading_zero_rows=zeros)
    return out


def require_complete_columns(tables):
    if not tables or len({t['dataset'] for t in tables}) != len(tables):
        raise ValueError('observation_table_coverage')
    if len({t.get('physical') for t in tables}) != len(tables):
        raise ValueError('observation_physical_table_collision')
    for t in tables:
        columns = t['columns']
        names = [c['name'] for c in columns]
        o = t.get('observation', {})
        if (o.get('protocol') != PROTOCOL or o.get('state') != 'complete'
                or sorted(o.get('expected_columns', [])) != sorted(names)
                or len(names) != len(set(names)) or not names):
            raise ValueError('column_observation_incomplete')
        if not o.get('snapshot') or not o.get('observed_at'):
            raise ValueError('column_observation_snapshot_missing')
        binding = table_binding(t)
        if o.get('binding') != binding or any(v is None for v in binding.values()):
            raise ValueError('column_observation_binding')
        if (len({c.get('physical_name') for c in columns}) != len(columns)
                or any(not c.get('physical_name') for c in columns)
                or not set(t.get('primary_key', [])) <= set(names)):
            raise ValueError('column_observation_mapping')
        rows = {c.get('profile', {}).get('rows') for c in columns}
        if len(rows) != 1:
            raise ValueError('column_observation_snapshot_mismatch')
        checked = profiles({'rows': next(iter(rows)),
                            'columns': [dict(c['profile'], name=c['name']) for c in columns]}, names)
        for c in columns:
            if c.get('profile') != checked[c['name']]:
                raise ValueError('column_observation_incomplete')
    return tables


def error_code(error):
    text = str(error)
    if re.fullmatch(r'read_only_aggregate_query_failed:[A-Z0-9]{5}', text):
        return text
    if isinstance(error, ValueError) and re.fullmatch(
            r'(?:oracle|ddl|observation|column|freshness|physical|ambiguous|table)_[a-z_]+', text):
        return text
    return 'observation_' + type(error).__name__.lower()


def write_report(path, report):
    if path is not None:
        # Audit output belongs to a new build directory, never a historical artifact.
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as f:
            f.write(json.dumps(report, sort_keys=True, ensure_ascii=False) + '\n')
