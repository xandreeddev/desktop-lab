"""Pure prerequisites/evidence for guest S3; no VM or power operations."""
import re
import xml.etree.ElementTree as ET


def require(value):
    if not value:
        raise ValueError('Native suspend prerequisite identity failed')


def domain_facts(raw):
    require(isinstance(raw, bytes) and len(raw) <= 1024 * 1024)
    root = ET.fromstring(raw)
    require(root.tag == 'domain' and root.findtext('name') == 'desktop-lab-lucent')
    node = root.find('./os/type')
    require(node is not None and node.text == 'hvm' and node.get('arch') == 'x86_64')
    pm = root.find('./pm/suspend-to-mem')
    require(pm is None or pm.get('enabled') in ('yes', 'no'))
    return {'acpi': root.find('./features/acpi') is not None,
            'machine': node.get('machine'),
            's3_advertisement': pm.get('enabled') if pm is not None else 'hypervisor-default'}


def prerequisites(domain, guest):
    require(type(domain['acpi']) is bool
            and domain['s3_advertisement'] in ('yes', 'no', 'hypervisor-default'))
    require(isinstance(guest['power_states'], list) and isinstance(guest['memory_modes'], list)
            and guest['logind_can_suspend'] in ('yes', 'no', 'na', 'challenge')
            and type(guest['sleep_block_inhibitors']) is int and 0 <= guest['sleep_block_inhibitors'] <= 256)
    blockers = []
    if domain['acpi'] is not True:
        blockers.append('domain_acpi_not_enabled')
    # An absent XML setting is not affirmative proof of the actual firmware's
    # S3 advertisement; live guest capability observation remains mandatory.
    if domain['s3_advertisement'] == 'no':
        blockers.append('domain_s3_advertisement_disabled')
    if 'mem' not in guest['power_states']:
        blockers.append('guest_mem_state_unavailable')
    if 'deep' not in guest['memory_modes']:
        blockers.append('guest_deep_s3_unavailable')
    if guest['selected_memory_mode'] != 'deep':
        blockers.append('guest_deep_s3_not_selected')
    if guest['logind_can_suspend'] != 'yes':
        blockers.append('regular_user_suspend_not_authorized_without_prompt')
    if guest['sleep_block_inhibitors'] != 0:
        blockers.append('sleep_block_inhibitor_present')
    return {'candidate_only': not blockers, 'blockers': blockers,
            'execution_authorized': False,
            'remaining': ['effective_systemd_sleep_policy_review', 'secure_lock_lifetime_admission',
                          'release_process_output_file_admission', 'independent_conditional_host_wake_owner',
                          'actual_s3_transition_and_resume_preservation']}


def memory_modes(raw):
    require(isinstance(raw, str) and len(raw) <= 4096)
    values, selected = [], []
    for token in raw.split():
        require(re.fullmatch(r'(s2idle|shallow|deep)|\[(s2idle|shallow|deep)\]', token))
        if token.startswith('['):
            selected.append(token[1:-1])
        values.append(token.strip('[]'))
    require(len(set(values)) == len(values) and len(selected) <= 1)
    return values, selected[0] if selected else None


def actual_s3_interval(before, after, events, suspended_status, kernel_deep_entry_and_exit):
    """Necessary transition evidence, not complete secure/preservation acceptance."""
    require(before['boot_id'] == after['boot_id'])
    require(all(type(value[key]) is int and value[key] >= 0
                for value in (before, after) for key in ('boottime_ns', 'monotonic_ns')))
    require(isinstance(events, list) and len(events) <= 64)
    require(events == ['SUSPEND', 'WAKEUP'] and suspended_status == 'suspended'
            and kernel_deep_entry_and_exit is True)
    gap = ((after['boottime_ns'] - before['boottime_ns'])
           - (after['monotonic_ns'] - before['monotonic_ns']))
    require(after['boottime_ns'] > before['boottime_ns']
            and after['monotonic_ns'] > before['monotonic_ns'] and gap >= 1_000_000_000)
    return {'guest_s3_interval_observed': True, 'sleep_clock_gap_ns': gap,
            'preservation_and_secure_lock_still_required': True}
