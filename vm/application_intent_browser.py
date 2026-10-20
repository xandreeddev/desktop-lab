"""Pure fresh-page identity policy for the owned application-intent browser."""
import re


def require(value):
    if not value:
        raise ValueError('Owned browser page identity failed')


def pages(values):
    require(isinstance(values, list) and len(values) <= 32)
    result = []
    for value in values:
        require(isinstance(value, dict))
        if value.get('type') != 'page':
            continue
        require(isinstance(value.get('id'), str) and re.fullmatch(r'[A-Za-z0-9-]{1,128}', value['id']))
        require(all(isinstance(value.get(key), str) and len(value[key].encode()) <= 8192
                    for key in ('url', 'title')))
        result.append({key: value[key] for key in ('id', 'url', 'title')})
    require(len({page['id'] for page in result}) == len(result))
    return sorted(result, key=lambda page: page['id'])


def fresh_documents(before, current, files, control):
    """Return the exact supplied one/two fresh documents, or None while pending.

    A preexisting matching URL/title is never evidence for this request. The
    original blank target must survive unchanged; no unexpected pages qualify.
    """
    require(1 <= len(files) <= 2 and len({item['uri'] for item in files}) == len(files))
    require(control['url'] == 'about:blank' and control in before)
    require(len({item['id'] for item in before}) == len(before)
            and len({item['id'] for item in current}) == len(current))
    if control not in current:
        return None
    old = {item['id'] for item in before}
    new = [item for item in current if item['id'] not in old]
    if len(new) != len(files) or len(current) != len(before) + len(files):
        return None
    if not all(item in current for item in before):
        return None
    if not all(sum(page['url'] == item['uri'] and page['title'] == item['title'] for page in new) == 1
               for item in files):
        return None
    return sorted(new, key=lambda page: page['id'])
