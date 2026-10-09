"""Pure menu definitions: parsing, extension merging, links and visible entries.

Callers provide condition evaluation. No filesystem, processes, UI or Omarchy
route policy lives in this module.
"""
import json
import re


def jsonc(text):
    # Protect quoted strings before removing comments and trailing commas.
    quoted = r'"(?:\\.|[^"\\])*"'
    text = re.sub(quoted + r'|//[^\n]*|/\*[\s\S]*?\*/',
                  lambda m: m[0] if m[0].startswith('"') else '', text)
    text = re.sub(quoted + r'|,\s*(?=[}\]])',
                  lambda m: m[0] if m[0].startswith('"') else '', text)
    data = json.loads(text or '{}')
    if not isinstance(data, dict):
        raise ValueError('Menu JSONC must contain an object')
    return data.get('items', data)


class Model:
    def __init__(self, defaults, extensions):
        self.items = {'root': {'label': 'Menu', 'parent': ''}}
        for source in (defaults, extensions):
            for key, value in source.items():
                if isinstance(value, dict):
                    self.items[key] = self.items.get(key, {}) | value
        for key, item in self.items.items():
            item.setdefault('label', key)
            item.setdefault('parent', key.rpartition('.')[0] or 'root')

    def resolve(self, route):
        route = (route or 'root').lower().replace('_', '-')
        for _ in range(len(self.items) + 1):
            if route not in self.items:
                for key, item in self.items.items():
                    aliases = item.get('aliases', [])
                    if isinstance(aliases, str):
                        aliases = [aliases]
                    if route in [a.lower().replace('_', '-') for a in aliases]:
                        route = key
                        break
                else:
                    raise ValueError('Unknown menu route: ' + route)
            target = self.items[route].get('target')
            if not target:
                return route
            route = target
        raise ValueError('Cycle in menu links')

    def rows(self, route, evaluate):
        rows = []
        for key, item in self.items.items():
            if item['parent'] != route or not evaluate(item.get('when'), True):
                continue
            disabled = evaluate(item.get('disabled'), False)
            checked = evaluate(item.get('checked'), False)
            detail = item.get('description', '')
            if disabled:
                detail = 'Unavailable / already installed' + (' · ' + detail if detail else '')
            elif checked:
                detail = 'Current' + (' · ' + detail if detail else '')
            elif not item.get('action'):
                detail = detail or 'Open submenu'
            rows.append({'label': item['label'], 'detail': detail, 'value': key, 'disabled': disabled})
        return rows
