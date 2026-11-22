#!/usr/bin/env python3
import dbus,dbus.service,dbus.mainloop.glib,sys,json
from gi.repository import GLib
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus=dbus.SessionBus();name=dbus.service.BusName('org.emberwm.TrayTest',bus=bus)
def log(kind,**args):
 with open(sys.argv[1],'a')as f:f.write(json.dumps({'kind':kind,**args})+'\n')
class Item(dbus.service.Object):
 @dbus.service.method('org.freedesktop.DBus.Properties',in_signature='ss',out_signature='v')
 def Get(self,interface,key):
  if key=='Title':return dbus.String('Ember tray fixture')
  if key=='Menu' and len(sys.argv)<3:return dbus.ObjectPath('/Menu')
  raise dbus.exceptions.DBusException('Unknown property')
 @dbus.service.method('org.kde.StatusNotifierItem',in_signature='ii',out_signature='')
 def Activate(self,x,y):log('activate')
 @dbus.service.method('org.kde.StatusNotifierItem',in_signature='ii',out_signature='')
 def ContextMenu(self,x,y):log('native-menu')
class Menu(dbus.service.Object):
 @dbus.service.method('com.canonical.dbusmenu',in_signature='i',out_signature='b')
 def AboutToShow(self,id):log('about',id=int(id));return True
 @dbus.service.method('com.canonical.dbusmenu',in_signature='iias',out_signature='u(ia{sv}av)')
 def GetLayout(self,id,depth,properties):
  children=[(11,{'label':'_Open','enabled':True}),(12,{'label':'Disabled','enabled':False}),(13,{'label':'More','children-display':'submenu'}),(14,{'type':'separator'})] if id==0 else [(21,{'label':'Nested action','enabled':True})]
  log('layout',id=int(id))
  return dbus.UInt32(1),dbus.Struct((dbus.Int32(id),dbus.Dictionary({},signature='sv'),dbus.Array([dbus.Struct((dbus.Int32(i),dbus.Dictionary(p,signature='sv'),dbus.Array([],signature='v')),signature='ia{sv}av',variant_level=1)for i,p in children],signature='v')),signature='ia{sv}av')
 @dbus.service.method('com.canonical.dbusmenu',in_signature='isvu',out_signature='')
 def Event(self,id,event,data,timestamp):log('event',id=int(id),event=str(event))
item=Item(bus,'/StatusNotifierItem');menu=Menu(bus,'/Menu')
watcher=dbus.Interface(bus.get_object('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher'),'org.kde.StatusNotifierWatcher');watcher.RegisterStatusNotifierItem('org.emberwm.TrayTest')
log('registered');GLib.MainLoop().run()
