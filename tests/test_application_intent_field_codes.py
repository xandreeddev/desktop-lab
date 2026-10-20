"""HTTP field-code cardinality and temporary owned entry writes; no socket/transport."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'vm'))

def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec);sys.modules[name]=module;spec.loader.exec_module(module)
    return module

browser=load('application_intent_browser',ROOT/'vm/application_intent_browser.py')
fixture=load('intent_field_code_fixture',ROOT/'vm/fixtures/application-intent-fixture.py')
driver=load('intent_field_code_driver',ROOT/'vm/test-application-intent.py')
from application_intent_policy import http_open_count
FILES=[{'name':'one.html','uri':'file:///owned/one.html','title':'Owned first'},
       {'name':'two.html','uri':'file:///owned/two.html','title':'Owned second'}]
MANIFEST={'run_id':'abcdef','http_mode':True,'http_field_code':'%u','files':FILES}

class SingularPolicy(unittest.TestCase):
    def test_default_plural_policy_and_routes_are_unchanged(self):
        self.assertEqual(http_open_count(False),2)
        self.assertEqual(http_open_count(True),2)
        plural=dict(MANIFEST);plural.pop('http_field_code')
        docs=fixture.http_documents(plural,12345)
        self.assertEqual(len(docs),2)
        self.assertEqual(docs[1],{'uri':'http://127.0.0.1:12345/abcdef/second%20document?literal=%24%28unused%29%3B#second',
                                  'title':'Owned second'})
        self.assertEqual(driver.expected_documents(plural,{'targets':docs},'open',True),docs)
        self.assertEqual(driver.expected_documents(plural,None,'open'),FILES)
        self.assertEqual(driver.expected_documents(plural,None,'action'),FILES)
        with self.assertRaises(ValueError):
            driver.expected_documents(plural,{'targets':docs[:1]},'open',True)

    def test_unknown_codes_wrong_types_and_singular_outside_http_are_rejected(self):
        for mode,code in [(False,'%u'),(True,'%f'),(True,'%z'),(True,''),(True,None),
                          (True,1),(1,'%U'),(None,'%U')]:
            with self.subTest(mode=mode,code=code),self.assertRaises(ValueError):
                http_open_count(mode,code)
        for manifest in [dict(MANIFEST,http_mode=False),dict(MANIFEST,http_field_code='%z')]:
            with self.subTest(manifest=manifest),self.assertRaises(ValueError):fixture.http_enabled(manifest)

    def test_host_cli_rejects_invalid_modes_before_loading_native_helpers(self):
        for extra in [['--http-field-code','%u'],['--http-uris','--http-field-code','%z']]:
            with self.subTest(extra=extra),mock.patch.object(sys,'argv',
                ['test-application-intent.py','--release-manifest','/not-read',*extra]),\
                mock.patch.object(driver,'load_helpers')as helpers,contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit)as error:driver.main()
                self.assertEqual(error.exception.code,2)
            helpers.assert_not_called()

    def test_guest_initialize_dispatch_preserves_old_calls_and_validates_explicit_code(self):
        for arguments,expected in [(['ab'*32],(False,'%U')),(['ab'*32,'http'],(True,'%U')),
                                   (['ab'*32,'http','%u'],(True,'%u'))]:
            with self.subTest(arguments=arguments),mock.patch.object(sys,'argv',
                ['fixture.py','initialize','/owned','abcdef',*arguments]),\
                mock.patch.object(fixture.signal,'alarm'),mock.patch.object(fixture,'initialize')as init:
                fixture.main()
                self.assertEqual(init.call_args.args,('/owned','abcdef','ab'*32,*expected))
        with mock.patch.object(fixture,'context')as context,self.assertRaises(ValueError):
            fixture.initialize('/not-created','abcdef','ab'*32,True,'%z')
        context.assert_not_called()

    def test_fixed_http_document_is_exactly_one_and_retains_encoded_literal_values(self):
        docs=fixture.http_documents(MANIFEST,12345)
        self.assertEqual(len(docs),1)
        self.assertEqual(docs[0]['title'],'Owned first')
        self.assertEqual(docs[0]['uri'],
            'http://127.0.0.1:12345/abcdef/first%20%25U%E9%9B%AA?encoded=%2520&field=%25u#first')
        for port in [True,0,1023,65536,12345.0,'12345']:
            with self.subTest(port=port),self.assertRaises(ValueError):fixture.http_documents(MANIFEST,port)
        changed=dict(MANIFEST,http_field_code='%z')
        with self.assertRaises(ValueError):fixture.http_documents(changed,12345)

    def test_exact_one_http_open_is_independent_of_two_local_action_targets(self):
        docs=fixture.http_documents(MANIFEST,12345)
        self.assertEqual(driver.expected_documents(MANIFEST,{'targets':docs},'open',True,'%u'),docs)
        self.assertEqual(driver.expected_documents(MANIFEST,{'targets':docs},'action',True,'%u'),FILES)
        for extra in [[],docs*2,FILES,None]:
            with self.subTest(extra=extra),self.assertRaises(ValueError):
                driver.expected_documents(MANIFEST,{'targets':extra},'open',True,'%u')
        with self.assertRaises(ValueError):driver.expected_documents(MANIFEST,{'targets':docs},'foreign',True,'%u')
        with self.assertRaises(ValueError):driver.expected_documents(dict(MANIFEST,files=FILES[:1]),{'targets':docs},'action',True,'%u')

    def test_guest_request_policy_keeps_action_two_and_http_open_one(self):
        docs=fixture.http_documents(MANIFEST,12345)
        with mock.patch.object(fixture,'http_authority',return_value={'targets':docs})as authority:
            self.assertEqual(fixture.request_documents('/owned','abcdef',MANIFEST,'action'),FILES)
            authority.assert_not_called()
            self.assertEqual(fixture.request_documents('/owned','abcdef',MANIFEST,'open'),docs)
            authority.assert_called_once()

    def test_guest_local_and_action_requests_reject_wrong_count_or_operation(self):
        with mock.patch.object(fixture,'http_authority')as authority:
            for http_mode,operation in [(False,'open'),(False,'action'),(True,'action')]:
                for files in [[],FILES[:1],FILES+FILES,None]:
                    manifest=dict(MANIFEST,http_mode=http_mode,http_field_code='%U',files=files)
                    with self.subTest(http_mode=http_mode,operation=operation,files=files),self.assertRaises(ValueError):
                        fixture.request_documents('/owned','abcdef',manifest,operation)
            with self.assertRaises(ValueError):
                fixture.request_documents('/owned','abcdef',MANIFEST,'foreign')
        authority.assert_not_called()

    def test_guest_execute_rejects_duplicate_foreign_and_two_targets_before_any_read_or_exec(self):
        docs=fixture.http_documents(MANIFEST,12345);uri=docs[0]['uri']
        with mock.patch.object(fixture,'context',return_value=(Path('/owned'),MANIFEST)),\
             mock.patch.object(fixture,'request_documents',return_value=docs),\
             mock.patch.object(fixture,'regular')as read,mock.patch.object(fixture.os,'execv')as execute:
            for values in [[uri,uri],['http://foreign/'],[uri,'http://127.0.0.1:12345/other']]:
                with self.subTest(values=values),self.assertRaises(ValueError):
                    fixture.execute('/owned','abcdef',['open',*values])
        read.assert_not_called();execute.assert_not_called()

    def test_fresh_single_document_requires_exact_new_id_url_title_and_original_control(self):
        control={'id':'control','url':'about:blank','title':'Control'}
        docs=fixture.http_documents(MANIFEST,12345)
        page={'id':'new',**{'url':docs[0]['uri'],'title':docs[0]['title']}}
        self.assertEqual(browser.fresh_documents([control],[control,page],docs,control),[page])
        for pages in [[page],[control],[control,dict(page,id='control')],
                      [control,dict(page,title='foreign')],
                      [control,dict(page,url='http://foreign/')],
                      [control,page,dict(page,id='extra')]]:
            with self.subTest(pages=pages):
                try:result=browser.fresh_documents([control],pages,docs,control)
                except ValueError:continue
                self.assertIsNone(result)
        self.assertIsNone(browser.fresh_documents([control,page],[control,page],docs,control))

    def test_two_document_action_still_requires_both_exact_fresh_pages(self):
        control={'id':'control','url':'about:blank','title':'Control'}
        pages=[{'id':str(index),'url':item['uri'],'title':item['title']} for index,item in enumerate(FILES)]
        self.assertIsNone(browser.fresh_documents([control],[control,pages[0]],FILES,control))
        self.assertEqual(browser.fresh_documents([control],[control,*pages],FILES,control),pages)
        with self.assertRaises(ValueError):browser.fresh_documents([control],[control],[],control)
        with self.assertRaises(ValueError):browser.fresh_documents([control],[control],FILES+FILES,control)

    def test_actual_initialize_writes_only_owned_entry_with_lowercase_field_code_and_literal_action(self):
        with tempfile.TemporaryDirectory()as temporary:
            home=Path(temporary);directory=home/'fixture';directory.mkdir()
            data=home/'data';(data/'applications').mkdir(parents=True)
            state=home/'.local/state/lucent';state.mkdir(parents=True)
            (state/'desktop.json').write_bytes(b'{"owned":"preferences"}')
            for name in ['fixture.py','owned_application_units.py','application_intent_policy.py',
                         'application_intent_browser.py','owned_chromium_lease.py']:
                (directory/name).write_bytes(b'owned test source')
            capture=io.StringIO()
            with mock.patch.object(fixture,'context',return_value=(directory,None)),\
                 mock.patch.object(Path,'home',return_value=home),\
                 mock.patch.object(Path,'is_file',return_value=True),\
                 mock.patch.object(fixture.os,'access',return_value=True),\
                 mock.patch.dict(os.environ,{'XDG_DATA_HOME':str(data)}),\
                 mock.patch.object(fixture.chromium,'startup',return_value={'mocked':True}),\
                 contextlib.redirect_stdout(capture):
                fixture.initialize(str(directory),'abcdef','ab'*32,True,'%u')
            created=list((data/'applications').iterdir());self.assertEqual(len(created),1)
            text=created[0].read_text();primary=text.split('[Desktop Action Probe]')[0]
            self.assertEqual(primary.count(' open %u\n'),1);self.assertNotIn(' open %U\n',primary)
            response=json.loads(capture.getvalue());self.assertEqual(response['http_field_code'],'%u')
            for item in response['files']:self.assertIn(fixture.encoded(item['uri']),text.split('[Desktop Action Probe]')[1])
            self.assertEqual((state/'desktop.json').read_bytes(),b'{"owned":"preferences"}')

    def test_fixture_refuses_singular_nonhttp_initialize_before_context(self):
        with mock.patch.object(fixture,'context')as context,self.assertRaises(ValueError):
            fixture.initialize('/not-created','abcdef','ab'*32,False,'%u')
        context.assert_not_called()


if __name__=='__main__':unittest.main()
