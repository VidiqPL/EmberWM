#include <X11/Xlib.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc,char**argv){
 if(argc!=3)return 2;Display*d=XOpenDisplay(NULL);if(!d)return 3;
 Window w=XCreateSimpleWindow(d,DefaultRootWindow(d),0,0,1,1,0,0,0);XSelectInput(d,w,PropertyChangeMask);
 Atom clipboard=XInternAtom(d,"CLIPBOARD",False),target=XInternAtom(d,argv[1],False),property=XInternAtom(d,"EMBER_TEST_DATA",False),incr=XInternAtom(d,"INCR",False);
 XConvertSelection(d,clipboard,target,property,w,CurrentTime);XFlush(d);FILE*f=fopen(argv[2],"wb");if(!f)return 4;
 int streaming=0;unsigned long total=0;
 for(;;){XEvent e;XNextEvent(d,&e);if(e.type==SelectionNotify){if(!e.xselection.property)return 5;}
 else if(!(streaming&&e.type==PropertyNotify&&e.xproperty.state==PropertyNewValue&&e.xproperty.atom==property))continue;
 Atom actual;int format;unsigned long n,after;unsigned char*data=NULL;
 XGetWindowProperty(d,w,property,0,16*1024*1024,True,AnyPropertyType,&actual,&format,&n,&after,&data);
 if(actual==incr){streaming=1;XDeleteProperty(d,w,property);XFlush(d);if(data)XFree(data);continue;}
 if(format!=8&&n)return 6;if(n){fwrite(data,1,n,f);total+=n;}if(data)XFree(data);XFlush(d);
 if(!streaming||!n)break;
 }
 fclose(f);printf("%lu\n",total);XDestroyWindow(d,w);XCloseDisplay(d);return 0;
}
