#!/usr/bin/env python3
"""Check actual static output for broken links/assets and missing Pages prefixes."""
import os
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[1] / 'dist'
BASE = '/' + os.environ.get('SITE_BASE_PATH', '').strip('/')
BASE = BASE.rstrip('/') + '/'


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.ids = set()
        self.links = []
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        for key in ('href', 'src'):
            if key in attrs:
                self.links.append(attrs[key])
        if 'srcset' in attrs:
            self.links.extend(item.strip().split()[0] for item in attrs['srcset'].split(','))


def main():
    pages = {path: Page(path) for path in ROOT.rglob('*.html')}
    if not pages:
        raise SystemExit('Build the site before checking it.')
    errors = []
    count = 0
    for path, page in pages.items():
        page_url = BASE + str(path.relative_to(ROOT)).removesuffix('index.html')
        for link in page.links:
            parsed = urlsplit(link)
            if parsed.scheme or parsed.netloc:
                continue
            resolved = urlsplit(urljoin(page_url, link))
            if not resolved.path.startswith(BASE):
                errors.append(f'{path.relative_to(ROOT)}: URL escapes base {BASE}: {link}')
                continue
            target = ROOT / unquote(resolved.path[len(BASE):])
            if target.is_dir():
                target /= 'index.html'
            if not target.is_file():
                errors.append(f'{path.relative_to(ROOT)}: missing target: {link}')
            elif resolved.fragment and target in pages and unquote(resolved.fragment) not in pages[target].ids:
                errors.append(f'{path.relative_to(ROOT)}: missing anchor: {link}')
            count += 1
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'Checked {len(pages)} pages and {count} links/assets under {BASE}.')


if __name__ == '__main__':
    main()
