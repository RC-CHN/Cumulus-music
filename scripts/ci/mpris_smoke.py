"""Linux MPRIS over a private D-Bus session, with real GTK/libmpv/null output.

Run under GDK_BACKEND=x11 LC_ALL=en_US.UTF-8 dbus-run-session -- xvfb-run -a.
Only the example process has fixture commands; no production IPC is replaced.
"""
import argparse
import json
import os
import select
from pathlib import Path
import subprocess
import time
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib

PATH = '/org/mpris/MediaPlayer2'
PLAYER = 'org.mpris.MediaPlayer2.Player'
PROPERTIES = 'org.freedesktop.DBus.Properties'

class PrivateBus:
    def __init__(self, root):
        self.address = 'unix:path=' + str(root/'bus.sock')
        self.log = (root/'bus.log').open('x')
        self.process = None
    def start(self):
        assert self.process is None
        self.process = subprocess.Popen(['dbus-daemon','--session','--nofork','--address='+self.address,'--print-address'], stdout=subprocess.PIPE,stderr=self.log)
        ready, _, _ = select.select([self.process.stdout], [], [], 5)
        assert ready and self.process.stdout.readline().strip(), 'private bus failed to start'
    def connect(self):
        value = Gio.DBusConnection.new_for_address_sync(self.address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
        value.set_exit_on_close(False)
        return value
    def stop(self):
        if self.process is not None:
            self.process.terminate()
            try: self.process.wait(timeout=5)
            except subprocess.TimeoutExpired: self.process.kill(); self.process.wait()
            self.process.stdout.close()
            self.process = None
    def close(self):
        self.stop()
        self.log.close()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    bus = PrivateBus(root)
    connection = None
    context = GLib.MainContext.default()
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(run)
    env['DBUS_SESSION_BUS_ADDRESS'] = bus.address
    env.pop('DBUS_STARTER_ADDRESS', None)
    env.pop('DBUS_STARTER_BUS_TYPE', None)
    for key, name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime')]:
        directory = root / name
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    changes, seeks, steps = [], [], []
    def drain():
        while context.pending(): context.iteration(False)
    def wait(condition, message, seconds=15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            drain()
            result = condition()
            if result: return result
            if process.poll() is not None: raise AssertionError(f'probe exited {process.returncode}: '+message)
            time.sleep(.025)
        raise AssertionError('timed out: '+message)
    def state(): return json.loads((run/'state.json').read_text())
    sequence = 0
    def control(kind):
        nonlocal sequence
        sequence += 1
        pending = run / 'command.pending'
        pending.write_text(json.dumps({'kind':kind,'id':sequence}))
        pending.replace(run/'command.json')
        reply = run/f'ack-{sequence}.json'
        wait(reply.exists, 'fixture '+kind)
        assert json.loads(reply.read_text())['ok'], kind
    def call(interface, name, parameters=None):
        result = connection.call_sync(service, PATH, interface, name, parameters, None, Gio.DBusCallFlags.NONE, 10000, None)
        drain()
        return result.unpack()
    def get(name): return call(PROPERTIES,'Get',GLib.Variant('(ss)',(PLAYER,name)))[0]
    def set_property(name, signature, value):
        return call(PROPERTIES,'Set',GLib.Variant('(ssv)',(PLAYER,name,GLib.Variant(signature,value))))
    def has_owner():
        return connection.call_sync('org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner',GLib.Variant('(s)',(service,)),None,Gio.DBusCallFlags.NONE,1000,None).unpack()[0]
    def reject(operation):
        try: operation()
        except GLib.Error: return
        raise AssertionError('invalid operation was acknowledged as successful')
    process = None
    subscriptions = []
    try:
        bus.start()
        connection = bus.connect()
        with (root/'host.log').open('x') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            wait(lambda:(run/'ready.json').exists() and (run/'state.json').exists(), 'native GUI')
            service = json.loads((run/'ready.json').read_text())['service']
            assert not has_owner() and state()['engines']==0
            steps.append('lazy-unselected')
            control('start')
            wait(has_owner,'MPRIS name')
            def changed(_c,_s,_p,_i,_n,parameters): changes.append(parameters.unpack())
            def seeked(_c,_s,_p,_i,_n,parameters): seeks.append(parameters.unpack()[0])
            subscriptions.append(connection.signal_subscribe(service,PROPERTIES,'PropertiesChanged',PATH,None,Gio.DBusSignalFlags.NONE,changed))
            subscriptions.append(connection.signal_subscribe(service,PLAYER,'Seeked',PATH,None,Gio.DBusSignalFlags.NONE,seeked))
            all_props = call(PROPERTIES,'GetAll',GLib.Variant('(s)',(PLAYER,)))[0]
            assert all_props['PlaybackStatus']=='Playing' and all_props['CanControl'] and all_props['CanSeek']
            metadata = all_props['Metadata']
            assert metadata['xesam:title']=='合成歌曲 2' and metadata['xesam:artist']==['测试歌手']
            assert metadata['mpris:length']==30_000_000, metadata
            assert 'url' not in str(metadata).lower() and 'SECRET' not in str(metadata)
            first_id = metadata['mpris:trackid']
            assert first_id.startswith('/io/cumulus/music/track/')
            before = get('Position')
            wait(lambda:get('Position') > before+250_000,'real PCM progress')
            steps.append('metadata-native-clock')
            call(PLAYER,'Pause')
            assert get('PlaybackStatus')=='Paused'
            paused = get('Position')
            time.sleep(.4)
            assert abs(get('Position')-paused)<40_000
            call(PLAYER,'SetPosition',GLib.Variant('(ox)',(first_id,4_000_000)))
            assert abs(get('Position')-4_000_000)<40_000
            wait(lambda:seeks and abs(seeks[-1]-4_000_000)<40_000,'Seeked signal')
            call(PLAYER,'SetPosition',GLib.Variant('(ox)',('/io/cumulus/stale',7_000_000)))
            assert abs(get('Position')-4_000_000)<40_000
            call(PLAYER,'SetPosition',GLib.Variant('(ox)',(first_id,40_000_000)))
            assert abs(get('Position')-4_000_000)<40_000
            control('short_seek')
            wait(lambda:seeks and abs(seeks[-1]-4_010_000)<1_000,'short UI seek signal')
            call(PLAYER,'Seek',GLib.Variant('(x)',(-20_000_000,)))
            assert get('Position')<40_000
            steps.append('pause-seek-stale-bounds')
            call(PLAYER,'Next')
            second_id = get('Metadata')['mpris:trackid']
            assert second_id!=first_id and get('PlaybackStatus')=='Paused'
            assert get('Metadata')['xesam:title']=='合成歌曲 3'
            paused = get('Position')
            time.sleep(.35)
            assert abs(get('Position')-paused)<40_000
            call(PLAYER,'Previous')
            assert get('Metadata')['mpris:trackid']==first_id and get('PlaybackStatus')=='Paused'
            steps.append('steps-keep-paused')
            time.sleep(.3)
            drain()
            before_preferences = len(seeks)
            set_property('Volume','d',.37)
            set_property('Rate','d',1.5)
            assert get('Volume')==.37 and get('Rate')==1.5 and get('PlaybackStatus')=='Paused'
            set_property('Shuffle','b',True)
            assert get('Shuffle') and get('LoopStatus')=='Playlist'
            set_property('LoopStatus','s','Track')
            assert get('LoopStatus')=='Track' and not get('Shuffle')
            set_property('LoopStatus','s','Playlist')
            reject(lambda:set_property('Rate','d',float('nan')))
            reject(lambda:set_property('Position','x',2))
            reject(lambda:call(PLAYER,'OpenUri',GLib.Variant('(s)',('file:///not-authorized',))))
            time.sleep(.3)
            drain()
            assert len(seeks)==before_preferences, 'preferences emitted a spurious Seeked'
            steps.append('properties-real-use-cases')
            control('reject')
            reject(lambda:set_property('Volume','d',.12))
            assert get('Volume')==.37
            wait(lambda:state()['volume']==37,'rollback in core')
            control('allow')
            set_property('Volume','d',.38)
            assert get('Volume')==.38
            steps.append('set-awaits-persistence-and-rolls-back')
            call(PLAYER,'Play')
            before = state()
            for subscription in subscriptions: connection.signal_unsubscribe(subscription)
            subscriptions.clear()
            bus.stop()
            wait(lambda:state()['position'] > before['position']+.3, 'PCM clock continues without bus')
            time.sleep(2.4)  # Include a failed OS reconnect while the bus is absent.
            assert process.poll() is None and state()['engines']==before['engines']
            bus.start()
            connection = bus.connect()
            wait(has_owner,'MPRIS reacquired after bus restart')
            assert get('Metadata')['mpris:trackid']==first_id
            assert state()['engines']==before['engines']
            assert get('Volume')==.38 and get('Rate')==1.5
            subscriptions.append(connection.signal_subscribe(service,PROPERTIES,'PropertiesChanged',PATH,None,Gio.DBusSignalFlags.NONE,changed))
            subscriptions.append(connection.signal_subscribe(service,PLAYER,'Seeked',PATH,None,Gio.DBusSignalFlags.NONE,seeked))
            steps.append('bus-loss-does-not-restart-audio-and-recovers')
            before_ui_seek = len(seeks)
            control('seek')
            # Observe the live output clock. null output's paused time-pos includes
            # buffered latency; see docs/native-seek-validation.txt. Exact paused
            # seek is independently verified on resume by native_audio_probe.
            wait(lambda:len(seeks)>before_ui_seek and 9_000_000<=seeks[-1]<10_000_000,'UI-originated Seeked signal')
            wait(lambda:9_100_000<get('Position')<10_000_000,'native clock after UI seek')
            call(PLAYER,'Pause')
            call(PLAYER,'Play')
            assert get('PlaybackStatus')=='Playing'
            set_property('Rate','d',0.0)
            assert get('PlaybackStatus')=='Paused' and get('Rate')==1.5
            call(PLAYER,'PlayPause')
            assert get('PlaybackStatus')=='Playing'
            call(PLAYER,'Stop')
            assert get('PlaybackStatus')=='Stopped' and get('Position')==0
            call(PLAYER,'Next')
            assert get('PlaybackStatus')=='Stopped'
            call(PLAYER,'Play')
            assert get('PlaybackStatus')=='Playing' and get('Position')<1_000_000
            steps.append('system-play-stop-state')
            control('hide')
            wait(lambda:not state()['visible'],'hidden native window')
            call('org.mpris.MediaPlayer2','Raise')
            wait(lambda:state()['visible'],'MPRIS Raise')
            steps.append('raise')
            control('switch')
            wait(lambda:not has_owner(),'account retirement removes old session')
            control('start')
            wait(has_owner,'new account MPRIS session')
            call(PLAYER,'Pause')
            new_id = get('Metadata')['mpris:trackid']
            assert new_id!=first_id
            paused = get('Position')
            call(PLAYER,'SetPosition',GLib.Variant('(ox)',(first_id,11_000_000)))
            assert abs(get('Position')-paused)<40_000
            assert get('Volume')==1.0 and get('Rate')==1.0
            steps.append('account-isolation-old-track-rejected')
            control('clear')
            wait(lambda:not has_owner(),'empty queue removes media session')
            assert all('Position' not in change[1] for change in changes)
            assert any('Volume' in change[1] for change in changes) and len(seeks)>=3
            steps.append('signal-contract-and-session-removal')
            control('exit')
            assert process.wait(timeout=10)==0
            report={'ok':True,'steps':steps,'actualTauri':True,'actualMpris':True,'nativePcmNull':True,'propertySignals':len(changes),'seekedSignals':len(seeks),'realAccountRequests':0,'physicalDesktopPanelVerified':False,'windowsSmtcVerified':False}
            (root/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
            print(json.dumps(report,ensure_ascii=False))
    except Exception:
        (root/'failed.json').write_text(json.dumps({'completedSteps':steps,'propertySignals':changes,'seekedSignals':seeks,'processExit':process.poll() if process is not None else None},ensure_ascii=False,indent=2)+'\n')
        raise
    finally:
        for subscription in subscriptions: connection.signal_unsubscribe(subscription)
        if process is not None and process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
        bus.close()

if __name__=='__main__': main()
