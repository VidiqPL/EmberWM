#define _POSIX_C_SOURCE 200809L
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/shape.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc,char **argv) {
    Display *d=XOpenDisplay(NULL);if(!d){fputs("XOpenDisplay failed\n",stderr);return 2;}
    int screen=DefaultScreen(d);
    if(argc>2&&!strcmp(argv[1],"hit-test")){
        Window frame=strtoul(argv[2],NULL,0),root=RootWindow(d,screen),parent,*children;unsigned count;
        for(;;){
            Window tree_root;
            if(!XQueryTree(d,frame,&tree_root,&parent,&children,&count))return 3;
            if(children)XFree(children);
            if(parent==root||parent==None)break;
            frame=parent;
        }
        Window pointer_root,child;int rx,ry,wx,wy;unsigned mask;
        XQueryPointer(d,root,&pointer_root,&child,&rx,&ry,&wx,&wy,&mask);
        printf("{\"hit\":%s,\"frame\":%lu,\"child\":%lu}\n",child==frame?"true":"false",frame,child);
        XCloseDisplay(d);return 0;
    }
    if(argc>1&&!strcmp(argv[1],"focus-state")){
        Window focus;int revert;XGetInputFocus(d,&focus,&revert);
        printf("{\"focus\":%lu}\n",focus);XCloseDisplay(d);return 0;
    }
    if(argc>1&&(!strcmp(argv[1],"focus-none")||!strcmp(argv[1],"focus-root"))){
        XSetInputFocus(d,!strcmp(argv[1],"focus-root")?RootWindow(d,screen):None,RevertToNone,CurrentTime);XSync(d,False);XCloseDisplay(d);return 0;
    }
    if(argc>2&&!strcmp(argv[1],"request")){
        XEvent e={0};e.xclient.type=ClientMessage;e.xclient.window=strtoul(argv[2],NULL,0);e.xclient.message_type=XInternAtom(d,"_NET_ACTIVE_WINDOW",False);e.xclient.format=32;e.xclient.data.l[0]=1;
        XSendEvent(d,RootWindow(d,screen),False,SubstructureNotifyMask|SubstructureRedirectMask,&e);XFlush(d);XCloseDisplay(d);return 0;
    }
    Window w=XCreateSimpleWindow(d,RootWindow(d,screen),40,50,420,260,0,0,0x345678);
    XClassHint hint={.res_name="ember-x11-test",.res_class="EmberX11Test"};XSetClassHint(d,w,&hint);
    XStoreName(d,w,"Ember X11 compatibility test");
    int orphan=argc>1&&!strcmp(argv[1],"orphan");
    if(orphan||(argc>1&&!strcmp(argv[1],"game"))){
        XClassHint h={.res_name="crash-fixture",.res_class="CrashFixture"};XSetClassHint(d,w,&h);
        if(orphan){XSetWindowAttributes a={.override_redirect=True};XChangeWindowAttributes(d,w,CWOverrideRedirect,&a);}
    }
    if(argc>1&&(!strcmp(argv[1],"overlay")||!strcmp(argv[1],"menu"))){
        int overlay=!strcmp(argv[1],"overlay");
        XClassHint o={.res_name=overlay?"linusware_overlay":"linusware",.res_class=overlay?"linusware_overlay":"LinusWare"};XSetClassHint(d,w,&o);XStoreName(d,w,o.res_class);
        if(overlay){XSetWindowAttributes a={.override_redirect=True};XChangeWindowAttributes(d,w,CWOverrideRedirect,&a);XShapeCombineRectangles(d,w,ShapeInput,0,0,NULL,0,ShapeSet,Unsorted);}
    }
    Atom close=XInternAtom(d,"WM_DELETE_WINDOW",False);Atom take=XInternAtom(d,"WM_TAKE_FOCUS",False);
    Atom protocols[]={close,take};
    int active=argc>1&&!strcmp(argv[1],"active");
    XSetWMProtocols(d,w,protocols,active?2:1);
    if(active){XWMHints h={.flags=InputHint,.input=False};XSetWMHints(d,w,&h);}
    XSelectInput(d,w,ExposureMask|KeyPressMask|ButtonPressMask|StructureNotifyMask);
    if(argc>1&&!strcmp(argv[1],"popup")) {XSetWindowAttributes a={.override_redirect=True};XChangeWindowAttributes(d,w,CWOverrideRedirect,&a);}
    XMapWindow(d,w);XFlush(d);
    if(orphan){XSetInputFocus(d,w,RevertToNone,CurrentTime);XFlush(d);}
    printf("{\"window\":%lu}\n",w);fflush(stdout);
    for(;;){XEvent e;XNextEvent(d,&e);
        if(e.type==ClientMessage&&(Atom)e.xclient.data.l[0]==close)break;
        if(e.type==ClientMessage&&(Atom)e.xclient.data.l[0]==take){
            XSetInputFocus(d,w,RevertToParent,(Time)e.xclient.data.l[1]);XFlush(d);
            puts("{\"take_focus\":true}");fflush(stdout);
        }
        if(e.type==Expose){XSetWindowBackground(d,w,0x345678);XClearWindow(d,w);XFlush(d);}
        if(e.type==ButtonPress){printf("{\"button\":%u}\n",e.xbutton.button);fflush(stdout);}
        if(e.type==KeyPress){printf("{\"key\":%u}\n",e.xkey.keycode);fflush(stdout);}
        if(e.type==ConfigureNotify){printf("{\"width\":%d,\"height\":%d}\n",e.xconfigure.width,e.xconfigure.height);fflush(stdout);}
    }
    XDestroyWindow(d,w);XCloseDisplay(d);return 0;
}
