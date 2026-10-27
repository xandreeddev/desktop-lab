"""Pure/tempfile tests. No Cargo, FFmpeg, proc, network, VM or GPU execution."""
import contextlib
import copy
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('full_demand_observer_tools', HERE/'run.py')
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


def allocation():
    return {'kind':'glibc_mallinfo2','available':True,'glibc_version':'2.42',
            'bytes':{'arena':64,'arena_accounted_in_use':17,'arena_free':47,'direct_mmap':99,
                     'malloc_accounted_in_use_total':116},
            'counts':{'arena_free_chunks':5,'direct_mmap_regions':3}}


def memory(moment, elapsed=0, stopped=False):
    value = {'moment':moment,'elapsed_ms':elapsed,'memory':{
        'rss_kib':50000,'anonymous_kib':40000,'rss_high_water_kib':60000,'swap_kib':0}}
    if stopped:
        value['allocator'] = allocation()
    return value


def case(mode='retarget',pid=41):
    phases = []
    for revision,(label,target) in enumerate(m.PROFILE['phases'],1):
        size = target or [3840,2160]
        extent = size[0]*size[1]*4
        start = (revision-1)*13000
        phases.append({'phase':label,'revision':revision,'requested_size':target,
            'worker_ordinal':1 if mode=='retarget' else revision,
            'target_ready_elapsed_ms':100,'phase_observed_ms':12000,'phase_request_clock_ms':start,
            'publications':12,'first_size':size,'last_size':size,'rgba_bytes':extent,
            'aspect_size':[3840,2160],'maximum_capacity_bytes':extent,'latest_capacity_bytes':extent,
            'first_pts_ns':0,'last_pts_ns':1000,'observed_loop_wraps_total':revision,
            'stale_publications_total':0,'all_admitted_pts_match_ffprobe':True,
            'memory_samples':[memory('before_target',start),memory('target_observed',start+100),
                              memory('during_target',start+12100)]})
    count = 1 if mode=='retarget' else 5
    return {'kind':'full_demand_observation','mode':mode,'pid':pid,'phases':phases,'play_calls':count,
        'exact_worker_joins':[{'worker_ordinal':ordinal,'stop_requested':True,'watch_joined':True,
                              'cancellation_observed':True,'join_elapsed_ms':1} for ordinal in range(1,count+1)],
        'stopped_allocator_observations':[memory('before_play',0,True),
            memory('after_join_latest_held',65000,True),memory('after_latest_dropped',65001,True)],
        'final_latest_capacity_before_drop':3840*2160*4,'consumer_held_pixel_capacity_after_drop':0,
        'renderer_used':False,'fps_measured':False,'whole_shell_memory_accepted':False,
        'native_presentation_accepted':False}


def probe():
    return {'streams':[{'codec_name':'h264','width':3840,'height':2160,'pix_fmt':'yuv420p',
        'r_frame_rate':'60/1','avg_frame_rate':'60/1','sample_aspect_ratio':'1:1','nb_frames':'240',
        'time_base':'1/15360'}], 'frames':[{'best_effort_timestamp':index*256} for index in range(240)]}


class Pure(unittest.TestCase):
    def test_default_is_inert_and_run_requires_all_explicit_paths(self):
        with patch.object(m,'run',side_effect=AssertionError('native')), contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(m.main([]),0)
        value=json.loads(output.getvalue())
        self.assertTrue(value['prepared_only'])
        self.assertFalse(value['artifact_bound'])
        self.assertFalse(value['native_actions'])
        with patch.object(m,'run',side_effect=AssertionError('native')):
            with self.assertRaisesRegex(ValueError,'explicit_hosted_paths_required'):
                m.main(['--run'])

    def test_hosted_context_refuses_wrong_actor_source_trigger_and_workflow_identity(self):
        env={'GITHUB_ACTIONS':'true','GITHUB_REPOSITORY':m.PROFILE['repository'],
             'GITHUB_ACTOR':m.PROFILE['actor'],'GITHUB_TRIGGERING_ACTOR':m.PROFILE['actor'],
             'GITHUB_EVENT_NAME':'push','GITHUB_REF':'refs/heads/'+m.PROFILE['branch'],
             'LUCENT_CI_IMAGE':m.PROFILE['container_image'],'GITHUB_SHA':'a'*40,
             'LUCENT_WORKFLOW_SHA':'a'*40,'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1'}
        self.assertEqual(m.context(env)['GITHUB_SHA'],'a'*40)
        for key in ('GITHUB_ACTOR','GITHUB_TRIGGERING_ACTOR','GITHUB_EVENT_NAME','GITHUB_REF',
                    'LUCENT_CI_IMAGE','LUCENT_WORKFLOW_SHA','GITHUB_RUN_ID'):
            value=dict(env);value[key]='wrong'
            with self.subTest(key=key),self.assertRaises(ValueError):m.context(value)

    def test_actual_budget_uses_smaller_of_host_and_cgroup_and_forbids_swap(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary); group=root/'group';group.mkdir(); info=root/'meminfo'
            for key,value in {'cpu.max':'300000 100000','memory.max':str(10*1024**3),
                              'memory.current':str(6*1024**3),'memory.swap.max':'0'}.items():
                (group/key).write_text(value)
            info.write_text('MemAvailable: 9437184 kB\n')
            self.assertEqual(m.resource_snapshot(group,info)['effective_available_mib'],4096)
            (group/'memory.current').write_text(str(1024**3));info.write_text('MemAvailable: 4194304 kB\n')
            self.assertEqual(m.resource_snapshot(group,info)['effective_available_mib'],4096)
            for key,value in [('cpu.max','max 100000'),('memory.swap.max','1073741824'),('memory.max','999')]:
                before=(group/key).read_text();(group/key).write_text(value)
                with self.subTest(key=key),self.assertRaises(ValueError):m.resource_snapshot(group,info)
                (group/key).write_text(before)

    def test_each_admission_requires_three_fresh_4500_MiB_samples(self):
        with patch.object(m,'resource_snapshot',side_effect=[{'effective_available_mib':5000}]*3) as query,patch.object(m.time,'sleep'):
            self.assertEqual(len(m.admit()),3);self.assertEqual(query.call_count,3)
        with patch.object(m,'resource_snapshot',side_effect=[{'effective_available_mib':5000},{'effective_available_mib':4499}]),patch.object(m.time,'sleep'):
            with self.assertRaisesRegex(ValueError,'fresh_4500MiB_admission_required'):m.admit()

    def test_allocator_and_compiler_overrides_cannot_change_comparison(self):
        allowed={'CARGO_TERM_COLOR':'never','RUSTUP_TOOLCHAIN':'1.99.0'}
        self.assertEqual(m.forbidden_environment(allowed),[])
        for key in ('LD_PRELOAD','GLIBC_TUNABLES','MALLOC_TRIM_THRESHOLD_','MALLOC_CONF',
                    'CARGO_TARGET_DIR','CARGO_BUILD_JOBS','RUSTFLAGS','CARGO_PROFILE_RELEASE_LTO'):
            self.assertEqual(m.forbidden_environment({**allowed,key:'value'}),[key])

    def test_test_gate_requires_individual_success_not_listing_ignored_or_total_only(self):
        raw='test first ... ok\ntest second ... private caught panic\nok\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out;\n'
        self.assertEqual(m.test_summary(raw,2,['first','second'])['passed'],2)
        for value in (raw.replace('second','other'),raw.replace('second ... private caught panic\nok','second ... ignored'),raw+raw,raw.replace('0 ignored','1 ignored')):
            with self.assertRaises(ValueError):m.test_summary(value,2,['first','second'])

    def test_frozen_actual_source_counts_and_names_remain_explicit_and_new_consumer_unexecuted(self):
        self.assertEqual(m.PROFILE['test_counts'],{'domain':37,'video':67,'observer':21,'consumer':11})
        for key in ('domain','video','observer'):
            self.assertEqual(len(m.PROFILE['required_test_names'][key]),m.PROFILE['test_counts'][key])
        self.assertEqual(len(m.PROFILE['consumer_test_names']),11)
        self.assertIsNone(m.PROFILE['artifact_binding'])
        self.assertIsNone(m.PROFILE['tools_commit_binding'])
        self.assertEqual(m.PROFILE['source_qualification']['workflow_result'],'failure')
        self.assertIn('Catalog omits',m.PROFILE['source_qualification']['integration_failure'])

    def test_ffprobe_requires_actual_four_k_sixty_h264_frame_pts_and_aspect(self):
        expected=m.probe_frames(probe())
        self.assertEqual(len(expected),240);self.assertEqual(expected[0],0)
        self.assertEqual(expected[60],1_000_000_000)
        with_empty_groups={**probe(),'programs':[],'stream_groups':[]}
        self.assertEqual(m.probe_frames(with_empty_groups),expected)
        for change in ('codec','size','rate','aspect','frames','pts','unexpected_program'):
            value=probe()
            if change=='codec':value['streams'][0]['codec_name']='vp9'
            elif change=='size':value['streams'][0]['width']=1920
            elif change=='rate':value['streams'][0]['avg_frame_rate']='30/1'
            elif change=='aspect':value['streams'][0]['sample_aspect_ratio']='2:1'
            elif change=='frames':value['frames'].pop()
            elif change=='pts':value['frames'][1]['best_effort_timestamp']=0
            else:value['programs']=[{'unknown':True}]
            with self.subTest(change=change),self.assertRaises(ValueError):m.probe_frames(value)

    def test_both_modes_require_same_full_source_cover_revision_sequence_and_exact_workers(self):
        for mode in ('retarget','reopen'):
            value=case(mode)
            self.assertEqual(m.validate_case(value,mode,41,[0,1000]),value)
            self.assertEqual([row['revision'] for row in value['phases']],[1,2,3,4,5])
            self.assertIsNone(value['phases'][0]['requested_size'])
            self.assertIsNone(value['phases'][-1]['requested_size'])

    def test_late_ready_missing_worker_join_reset_revision_wrong_bytes_aspect_or_pts_fail(self):
        for change in ('ready_boundary','ready_late','join','cancel','extra_worker','revision',
                       'source_is_absence','bytes','aspect','pts','clock','duration','loop'):
            value=case()
            if change=='ready_boundary':value['phases'][1]['target_ready_elapsed_ms']=5000
            elif change=='ready_late':value['phases'][1]['target_ready_elapsed_ms']=6000
            elif change=='join':value['exact_worker_joins'][0]['watch_joined']=False
            elif change=='cancel':value['exact_worker_joins'][0]['cancellation_observed']=False
            elif change=='extra_worker':value['play_calls']=5
            elif change=='revision':value['phases'][4]['revision']=1
            elif change=='source_is_absence':value['phases'][0]['requested_size']='unavailable'
            elif change=='bytes':value['phases'][0]['rgba_bytes']=1920*1080*4
            elif change=='aspect':value['phases'][2]['aspect_size']=[16,9]
            elif change=='pts':value['phases'][2]['last_pts_ns']=500
            elif change=='clock':value['phases'][3]['phase_request_clock_ms']=0
            elif change=='duration':value['phases'][3]['phase_observed_ms']=11000
            else:value['phases'][-1]['observed_loop_wraps_total']=0
            with self.subTest(change=change),self.assertRaises(ValueError):m.validate_case(value,'retarget',41,[0,1000])

    def test_allocator_is_present_only_at_three_stopped_points_never_during_target(self):
        value=case();m.validate_case(value,'retarget',41,[0,1000])
        for change in ('during','missing','extra','swap','false_total','private'):
            value=case()
            if change=='during':value['phases'][0]['memory_samples'][0]['allocator']=allocation()
            elif change=='missing':del value['stopped_allocator_observations'][1]['allocator']
            elif change=='extra':value['stopped_allocator_observations'].append(memory('extra',65002,True))
            elif change=='swap':value['phases'][0]['memory_samples'][0]['memory']['swap_kib']=4
            elif change=='false_total':value['stopped_allocator_observations'][0]['allocator']['bytes']['malloc_accounted_in_use_total']=0
            else:value['phases'][0]['memory_samples'][0]['memory']['private_path']='/private'
            with self.subTest(change=change),self.assertRaises(ValueError):m.validate_case(value,'retarget',41,[0,1000])

    def test_allocator_unavailable_is_explicit_not_invented_zero_or_retention_result(self):
        for reason in ('unsupported_target','invalid_runtime_version','unsupported_glibc_version','counter_out_of_range'):
            value=case();value['stopped_allocator_observations'][1]['allocator']={
                'kind':'glibc_mallinfo2','available':False,'reason':reason}
            m.validate_case(value,'retarget',41,[0,1000])
        value=allocation();value['bytes']['arena_accounted_in_use']=2**64-1
        value['bytes']['malloc_accounted_in_use_total']=None
        m.allocator(value)

    def test_diagnostic_cannot_claim_renderer_fps_shell_or_presentation_acceptance(self):
        for flag in ('renderer_used','fps_measured','whole_shell_memory_accepted','native_presentation_accepted'):
            value=case();value[flag]=True
            with self.subTest(flag=flag),self.assertRaises(ValueError):m.validate_case(value,'retarget',41,[0,1000])
        value=case();value['private_uri']='https://private.invalid'
        with self.assertRaises(ValueError):m.validate_case(value,'retarget',41,[0,1000])

    def test_build_copy_changes_only_explicit_consumer_target_and_files_with_original_lock(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);tools=root/'tools';tools.mkdir();source=root/'source';source.mkdir()
            for name in m.CONSUMER_FILES:(tools/name).write_text('// consumer fixture\n')
            def archive(argv,**unused):
                name=next(part.split('=',1)[1] for part in argv if part.startswith('--output='))
                with tarfile.open(name,'x') as stream:
                    for path,raw in [('lucent/Cargo.lock',b'version = 4\n'),
                                     ('lucent/crates/lucent-services/Cargo.toml',b'[package]\nname="fixture"\n'),
                                     ('lucent/crates/lucent-services/src/lib.rs',b'// backend\n')]:
                        info=tarfile.TarInfo(path);info.size=len(raw);stream.addfile(info,io.BytesIO(raw))
            with patch.object(m.subprocess,'run',side_effect=archive):
                roster,record=m.prepare_consumer(source,tools,root/'scratch')
            self.assertEqual(record['backend_file_count'],3)
            self.assertEqual((root/'scratch/lucent/Cargo.lock').read_text(),'version = 4\n')
            self.assertEqual(m.staged_roster(root/'scratch'),roster)
            (root/'scratch/lucent/crates/lucent-services/src/lib.rs').write_text('// changed\n')
            self.assertNotEqual(m.staged_roster(root/'scratch'),roster)

    def test_source_archive_rejects_links_and_escape_before_extraction(self):
        for escape in (True,False):
            with tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary)
                def archive(argv,**unused):
                    name=next(part.split('=',1)[1] for part in argv if part.startswith('--output='))
                    with tarfile.open(name,'x') as stream:
                        info=tarfile.TarInfo('../escape' if escape else 'link')
                        if not escape:info.type=tarfile.SYMTYPE;info.linkname='/private'
                        stream.addfile(info)
                with patch.object(m.subprocess,'run',side_effect=archive),self.assertRaises(ValueError):
                    m.prepare_consumer(root,root,root/'scratch')

    def test_upload_uses_one_dedicated_envelope_directory_and_pinned_owner_limited_budget(self):
        workflow=(HERE.parents[1]/'.github/workflows/full-demand-observer.yml').read_text()
        self.assertIn('path: ${{ runner.temp }}/full-demand-observer-envelope/',workflow)
        self.assertNotIn('path: |',workflow)
        self.assertIn('--cpus 3 --memory 10g --memory-swap 10g',workflow)
        self.assertIn('ref: '+m.PROFILE['source_commit'],workflow)
        self.assertNotIn('workflow_dispatch',workflow)
        self.assertNotIn('pull_request',workflow)
        normal=(HERE.parents[1]/'.github/workflows/check.yml').read_text()
        self.assertIn('branches-ignore: [ci/lucent-full-demand-observer]',normal)
        self.assertIn('  pull_request:',normal)


if __name__ == '__main__':
    unittest.main()
