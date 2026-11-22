#!/usr/bin/env python3
"""Real D-Bus calls against an isolated fake NetworkManager; never host networking.
Run: dbus-run-session -- python3 tests/wifi-backend.py
"""
import json, os, pwd, shutil, subprocess, tempfile, time
from pathlib import Path
from gi.repository import Gio, GLib

ROOT=Path(__file__).resolve().parents[1]
address=os.environ['DBUS_SESSION_BUS_ADDRESS']
assert address.startswith('unix:path=/tmp/dbus-'), 'Run under dbus-run-session'
NM='org.freedesktop.NetworkManager'; BASE='/org/freedesktop/NetworkManager'
bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
bus.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','RequestName',GLib.Variant('(su)',(NM,4)),None,Gio.DBusCallFlags.NONE,3000,None)
profile={}; active_state=2; calls=[]
xml=f'''<node>
<interface name="{NM}">
<method name="GetDeviceByIpIface"><arg type="s" direction="in"/><arg type="o" direction="out"/></method>
<method name="ActivateConnection"><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/></method>
<method name="AddAndActivateConnection"><arg type="a{{sa{{sv}}}}" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/><arg type="o" direction="out"/></method>
</interface>
<interface name="{NM}.Settings"><method name="ListConnections"><arg type="ao" direction="out"/></method></interface>
<interface name="{NM}.Settings.Connection">
<method name="GetSettings"><arg type="a{{sa{{sv}}}}" direction="out"/></method>
<method name="Update"><arg type="a{{sa{{sv}}}}" direction="in"/></method>
</interface>
<interface name="{NM}.Device"><method name="Disconnect"/></interface>
<interface name="{NM}.Connection.Active"><property name="State" type="u" access="read"/></interface>
</node>'''
interfaces=Gio.DBusNodeInfo.new_for_xml(xml).interfaces

def method(_bus,_sender,_path,_iface,member,params,invocation):
    global profile,active_state
    calls.append(member)
    try:
        if member=='GetDeviceByIpIface':
            assert params.unpack()[0]=='wlan0'; reply=GLib.Variant('(o)',(BASE+'/Devices/1',))
        elif member=='ListConnections': reply=GLib.Variant('(ao)',([BASE+'/Settings/1'] if profile else [],))
        elif member=='GetSettings': reply=GLib.Variant('(a{sa{sv}})',(profile,))
        elif member in ('AddAndActivateConnection','Update'):
            settings={}
            groups=params.get_child_value(0)
            for i in range(groups.n_children()):
                group=groups.get_child_value(i);name=group.get_child_value(0).get_string();props=group.get_child_value(1)
                settings[name]={}
                for j in range(props.n_children()):
                    entry=props.get_child_value(j)
                    settings[name][entry.get_child_value(0).get_string()]=entry.get_child_value(1).get_variant()
            if settings['connection']['id'].unpack()=='Denied':
                invocation.return_dbus_error(NM+'.PermissionDenied','fixture denial');return
            assert settings['802-11-wireless-security']['key-mgmt'].unpack()=='wpa-psk'
            assert settings['connection']['permissions'].unpack()==['user:'+pwd.getpwuid(os.getuid()).pw_name+':']
            password=settings['802-11-wireless-security']['psk'].unpack()
            assert password in ('test-only-secret','wrong-password')
            if member=='Update': assert settings['ipv4']['dns'].unpack()==[0x01010101], 'Existing DNS settings lost'
            profile=settings
            profile['ipv4']['dns']=GLib.Variant('au',[0x01010101])
            active_state=4 if password=='wrong-password' else 2
            reply=GLib.Variant('(oo)',(BASE+'/Settings/1',BASE+'/ActiveConnection/1')) if member=='AddAndActivateConnection' else GLib.Variant('()',())
        elif member=='ActivateConnection': reply=GLib.Variant('(o)',(BASE+'/ActiveConnection/1',))
        elif member=='Disconnect': reply=GLib.Variant('()',())
        else: raise AssertionError(member)
        invocation.return_value(reply)
    except Exception as error:
        # Test failures report only type, avoiding accidental secret output.
        invocation.return_dbus_error(NM+'.FixtureFailure',type(error).__name__)
        raise

def get_property(_bus,_sender,_path,_interface,property):
    assert property=='State';return GLib.Variant('u',active_state)

for interface,path in zip(interfaces,[BASE,BASE+'/Settings',BASE+'/Settings/1',BASE+'/Devices/1',BASE+'/ActiveConnection/1']):
    bus.register_object(path,interface,method,get_property,None)

with tempfile.TemporaryDirectory(prefix='ember-wifi-backend-') as folder:
    tmp=Path(folder);shutil.copy2(ROOT/'tests/wifi-fixture-nmcli.sh',tmp/'nmcli');(tmp/'nmcli').chmod(0o755)
    env=os.environ.copy();env.update(DBUS_SYSTEM_BUS_ADDRESS=address,EMBER_WIFI_TEST_FIXTURE='private-bus',PATH=str(tmp)+':'+env['PATH'])
    proc=subprocess.Popen(['cargo','test','--locked','--offline','private_network_manager_contract','--','--ignored','--exact','wifi::network::tests::private_network_manager_contract'],cwd=ROOT,env=env)
    deadline=time.monotonic()+90
    try:
        while proc.poll() is None and time.monotonic()<deadline:
            while GLib.MainContext.default().pending():GLib.MainContext.default().iteration(False)
            time.sleep(.01)
        assert proc.wait(timeout=1)==0
        assert calls.count('AddAndActivateConnection')==2 and calls.count('Update')==2 and calls.count('Disconnect')==1,calls
        result={'passed':['Escaped nmcli scan parsing','New user-owned WPA2 connection via real D-Bus serialization','Saved credentials reused','Replacement PSK preserves DNS','Failed activation returns a safe error','Disconnect targets the selected adapter','Permission denial surfaced'],'scope':'Private dbus-run-session fake NetworkManager and fake nmcli; no host network mutations'}
        (ROOT/'docs/wifi-backend-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        if proc.poll() is None:proc.terminate();proc.wait(timeout=5)
