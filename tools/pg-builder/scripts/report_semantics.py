"""Extract bound aggregate proposals from stored reports; never execute report SQL."""
import json
import re

from library_metrics import digest, text
from library_observation import tokens, group_end


def ident(token):
    if token.startswith('`') and token.endswith('`'):
        token = token[1:-1].replace('``', '`')
    return token if re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', token) else None


def projections(parts):
    if not parts or parts[0].lower() != 'select':
        raise ValueError('not_select')
    depth, start, items = 0, 1, []
    for index in range(1, len(parts)):
        token = parts[index]
        if depth == 0 and token.lower() == 'from':
            items.append(parts[start:index])
            return items, index + 1
        if token == '(':
            depth += 1
        elif token == ')':
            depth -= 1
        if not 0 <= depth <= 32:
            raise ValueError('projection_depth')
        if depth == 0 and token == ',':
            items.append(parts[start:index])
            start = index + 1
    raise ValueError('from_missing')


def primary_table(parts, start, tables, source_schema):
    name = ident(parts[start]) if start < len(parts) else None
    if not name:
        raise ValueError('unsupported_primary_relation')
    index = start + 1
    if index < len(parts) and parts[index] == '.':
        if index + 1 >= len(parts) or not ident(parts[index + 1]):
            raise ValueError('unsupported_primary_relation')
        name += '.' + ident(parts[index + 1])
        index += 2
    qualified = name if '.' in name else source_schema + '.' + name
    matches = [t for t in tables if t['dataset'] == qualified]
    if len(matches) != 1:
        raise ValueError('primary_dataset_unbound')
    alias = name.split('.')[-1]
    if index < len(parts) and parts[index].lower() == 'as':
        index += 1
        alias = ident(parts[index]) if index < len(parts) else None
        if not alias:
            raise ValueError('primary_alias_invalid')
    elif index < len(parts) and parts[index].lower() not in {
            'join', 'left', 'right', 'inner', 'full', 'where', 'group', 'order', 'limit', ';'}:
        alias = ident(parts[index])
        if not alias:
            raise ValueError('primary_alias_invalid')
    return matches[0], alias


def column_reference(parts, table, alias):
    names = []
    for index, token in enumerate(parts):
        if index % 2:
            if token != '.':
                raise ValueError('expression_reference')
        else:
            name = ident(token)
            if not name:
                raise ValueError('expression_reference')
            names.append(name)
    if not names or len(parts) != len(names) * 2 - 1:
        raise ValueError('expression_reference')
    if len(names) == 2 and names[0] not in {alias, table['dataset'].split('.')[-1]}:
        raise ValueError('cross_dataset_expression_pending')
    if len(names) == 3 and '.'.join(names[:2]) != table['dataset']:
        raise ValueError('cross_dataset_expression_pending')
    if len(names) > 3:
        raise ValueError('expression_reference')
    columns = [c for c in table['columns'] if c.get('source_name', c['name']) == names[-1]]
    if len(columns) != 1:
        raise ValueError('expression_column_unbound')
    return columns[0]


def additive_expression(parts, table, alias):
    """Only bound column references and +/-; no literals, functions or hidden SQL."""
    segments, operators, start = [], [], 0
    for index, token in enumerate(parts):
        if token in ('+', '-'):
            segments.append(parts[start:index])
            operators.append(token)
            start = index + 1
    segments.append(parts[start:])
    columns = [column_reference(segment, table, alias) for segment in segments]
    if len(columns) > 32:
        raise ValueError('expression_budget')
    rendered = columns[0]['name']
    for operator, column in zip(operators, columns[1:]):
        rendered += ' ' + operator + ' ' + column['name']
    return rendered, columns


def derive(facts):
    reports = facts.get('report_definitions_observation')
    if reports is None:
        return {'metrics': [], 'audit': []}
    tables = facts['tables']
    source_schema = reports['dataset'].split('.')[0]
    result, audit = {}, []
    # Keep primary metric IDs stable; totals use the same bounded parser independently.
    sources = ((field, row) for field in ('sql_text', 'total_sql_text')
               for row in reports['definitions'] if field == 'sql_text' or row.get(field))
    for source_field, row in sources:
        row_hash = digest(row)
        entry = {'report_sha256': row_hash, 'source_field': source_field,
                 'state': 'pending', 'proposed': [], 'omitted': []}
        sql = row.get(source_field)
        if not sql or row.get('active') != '1':
            entry['reason'] = 'inactive_report' if row.get('active') != '1' else 'missing_sql_definition'
            audit.append(entry)
            continue
        try:
            ui = json.loads(row.get('data_list_config') or '[]')
            if not isinstance(ui, list) or len(ui) > 1024:
                raise ValueError('report_ui_shape')
            labels = {}
            for column in ui:
                if not isinstance(column, dict):
                    raise ValueError('report_ui_shape')
                alias, label = column.get('prop'), column.get('label')
                if not isinstance(alias, str) or not text(label, 256) or re.search(r'[\x00-\x1f]', label):
                    continue
                if alias in labels and labels[alias] != label:
                    raise ValueError('report_label_conflict')
                labels[alias] = label
            parts = tokens(sql)
            # A saved SQL fragment may intentionally omit WHERE/GROUP BY. No tail is executed.
            if any(p.lower() in {'union', 'intersect', 'except', 'delete', 'update', 'insert', 'into'}
                   for p in parts) or ';' in parts[:-1]:
                raise ValueError('report_statement_unsupported')
            items, start = projections(parts)
            table, table_alias = primary_table(parts, start, tables, source_schema)
            times = []
            for item in items:
                if item and item[0].lower() == 'date_format' and item[1:2] == ['(']:
                    try:
                        end = group_end(item, 1)
                        comma = item.index(',')
                        if (item[comma + 1:end] == ["'%Y-%m-%d'"]
                                and len(item[end + 1:]) == 2 and item[end + 1].lower() == 'as'):
                            column = column_reference(item[2:comma], table, table_alias)
                            if re.match(r'^(date|timestamp)', column['data_type']):
                                times.append(column['name'])
                    except (ValueError, IndexError):
                        continue
            for item in items:
                if not item or item[0].lower() not in {'sum', 'count', 'min', 'max', 'avg'}:
                    continue
                try:
                    if item[1:2] != ['(']:
                        raise ValueError('aggregate_shape')
                    end = group_end(item, 1)
                    if len(item[end + 1:]) != 2 or item[end + 1].lower() != 'as':
                        raise ValueError('aggregate_alias')
                    alias = ident(item[end + 2])
                    label = labels.get(alias)
                    if not label:
                        raise ValueError('aggregate_label_missing')
                    inner = item[2:end]
                    if inner == ['*'] and item[0].lower() == 'count':
                        expression, columns = '*', []
                    else:
                        expression, columns = additive_expression(inner, table, table_alias)
                    if item[0].lower() in {'sum', 'avg'} and any(not re.match(
                            r'^(smallint|integer|bigint|numeric|decimal|real|double)', c['data_type'])
                            for c in columns):
                        raise ValueError('aggregate_type_unbound')
                    formula = item[0].upper() + '(' + expression + ')'
                    identity = {'dataset': table['dataset'], 'alias': alias, 'formula': formula,
                                'module': row.get('module_name'), 'label': label}
                    name = 'report_' + digest(identity)[:20]
                    proposal = {
                        'dataset': table['dataset'], 'metric': name, 'label': label, 'synonyms': [label],
                        'description': 'Stored report aggregate ' + str(row.get('module_name', '')) +
                            ' / ' + alias + ': ' + formula,
                        # Reuse the bound metric ID rather than a generic alias that
                        # may collide with a hidden column in the role closure.
                        'sql': 'SELECT ' + formula + ' AS ' + name + ' FROM ' + table['dataset'],
                        'grain': ['Primary dataset records; verify report JOIN cardinality before certification.'],
                        'caveats': [
                            'Report-derived proposal, not an approved business definition or executed stored SQL.',
                            'Report configuration scope: ' + json.dumps(reports['scope'], sort_keys=True),
                            'Runtime tenant/company filters, unit, exclusions and JOIN equivalence require review.',
                            'Coverage is local_imported_snapshot_only, not complete enterprise history.',
                        ],
                    }
                    if len(set(times)) == 1:
                        proposal['time_dimension'] = table['dataset'] + '.' + times[0]
                    if source_field == 'total_sql_text' and name in result and result[name] != proposal:
                        # Do not borrow a primary date projection or discard a distinct total.
                        name = 'report_' + digest(dict(identity, source_field=source_field,
                                                       time_dimension=proposal.get('time_dimension')))[:20]
                        proposal['metric'] = name
                        proposal['sql'] = 'SELECT ' + formula + ' AS ' + name + ' FROM ' + table['dataset']
                    if name in result and result[name] != proposal:
                        raise ValueError('report_definition_conflict')
                    result[name] = proposal
                    entry['proposed'].append({'metric': name, 'alias': alias, 'formula': formula})
                except (ValueError, IndexError) as error:
                    entry['omitted'].append({'reason': str(error) if isinstance(error, ValueError) else 'aggregate_shape',
                                             'projection_sha256': digest(item)})
            entry['state'] = 'candidates_extracted' if entry['proposed'] else 'no_supported_aggregates'
        except (ValueError, TypeError, KeyError, IndexError):
            entry['state'] = 'unsupported_report'
        audit.append(entry)
    return {'metrics': [result[k] for k in sorted(result)], 'audit': audit}
