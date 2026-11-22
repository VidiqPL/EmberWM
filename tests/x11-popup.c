#define _POSIX_C_SOURCE 200809L
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/Xatom.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
static void log_event(const char*s){puts(s);fflush(stdout);}
int main(int argc,char**argv){
 int ewmh=argc>1&&!strcmp(argv[1],"ewmh");
 Display*d=XOpenDisplay(NULL);if(!d)return 2;
 Window root=DefaultRootWindow(d),main=XCreateSimpleWindow(d,root,40,50,600,400,0,0,0x345678),popup=0;
 Atom active=XInternAtom(d,"_NET_ACTIVE_WINDOW",False);if(ewmh)XSelectInput(d,root,PropertyChangeMask);
 XClassHint cls={.res_name="popup-fixture",.res_class="PopupFixture"};XSetClassHint(d,main,&cls);XStoreName(d,main,"Popup focus fixture");
 XSelectInput(d,main,ButtonPressMask|StructureNotifyMask);XMapWindow(d,main);XFlush(d);
 for(;;){XEvent e;XNextEvent(d,&e);
  if(ewmh&&popup&&e.type==PropertyNotify&&e.xproperty.window==root&&e.xproperty.atom==active){
   Atom type;int format;unsigned long count,remaining;unsigned char*bytes=NULL;
   if(XGetWindowProperty(d,root,active,0,1,False,XA_WINDOW,&type,&format,&count,&remaining,&bytes)==Success&&count&&bytes){
    Window target=*(unsigned long*)bytes;XFree(bytes);
    if(target!=main){log_event("popup-dismissed-by-active-window");XSetInputFocus(d,main,RevertToParent,CurrentTime);XDestroyWindow(d,popup);popup=0;XFlush(d);}
   }
  }else if(e.type==ButtonPress&&e.xbutton.window==main&&!popup){
   popup=XCreateSimpleWindow(d,root,100,100,260,160,0,0,0x884466);
   XSetWindowAttributes attrs={.override_redirect=True};XChangeWindowAttributes(d,popup,CWOverrideRedirect,&attrs);
   XClassHint hint={.res_name="popup-fixture",.res_class="PopupFixtureMenu"};XSetClassHint(d,popup,&hint);XStoreName(d,popup,"Friends dropdown fixture");
   Atom type=XInternAtom(d,"_NET_WM_WINDOW_TYPE",False),kind=XInternAtom(d,"_NET_WM_WINDOW_TYPE_POPUP_MENU",False);XChangeProperty(d,popup,type,XA_ATOM,32,PropModeReplace,(unsigned char*)&kind,1);XSetTransientForHint(d,popup,main);
   XSelectInput(d,popup,FocusChangeMask|ButtonPressMask);XMapWindow(d,popup);XSetInputFocus(d,popup,RevertToParent,CurrentTime);XFlush(d);log_event("popup-open");
   XEvent activate={0};activate.xclient.type=ClientMessage;activate.xclient.window=main;activate.xclient.message_type=active;activate.xclient.format=32;activate.xclient.data.l[0]=1;
   XSendEvent(d,root,False,SubstructureNotifyMask|SubstructureRedirectMask,&activate);XFlush(d);log_event("parent-activation-requested");
  }else if(e.type==FocusOut&&e.xfocus.window==popup&&e.xfocus.mode==NotifyNormal){
   log_event("popup-dismissed-by-focus");XDestroyWindow(d,popup);popup=0;XFlush(d);
  }else if(e.type==ButtonPress&&e.xbutton.window==popup){
   log_event("popup-item-clicked");
   Window dialog=XCreateSimpleWindow(d,root,420,240,340,220,0,0,0x225566);
   XClassHint hint={.res_name="friend-dialog",.res_class="FriendDialog"};XSetClassHint(d,dialog,&hint);XStoreName(d,dialog,"Add friend fixture");XSetTransientForHint(d,dialog,main);
   XSetInputFocus(d,main,RevertToParent,CurrentTime);XDestroyWindow(d,popup);popup=0;XMapWindow(d,dialog);XFlush(d);log_event("friend-dialog-open");
  }
 }
}
