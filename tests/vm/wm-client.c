#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "xdg-shell-client.h"
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *shell;
static struct wl_surface *surface;
static struct xdg_surface *xdg;
static struct xdg_toplevel *top;
static struct wl_keyboard *keyboard;
static bool running=true,fullscreen=false,test_pattern=false,log_keys=false;
static int width=320,height=240;
static unsigned f11_keys=0;
struct pixels {struct wl_buffer *buffer;void *data;size_t length;};
static void release(void *data,struct wl_buffer *buffer){struct pixels *p=data;wl_buffer_destroy(buffer);munmap(p->data,p->length);free(p);}
static const struct wl_buffer_listener buffer_listener={release};
static void paint(void){
    if(width<1||height<1||width>8192||height>8192)exit(8);
    char name[]="/tmp/ember-wm-pixels-XXXXXX";int fd=mkstemp(name);if(fd<0)exit(4);unlink(name);
    size_t bytes=(size_t)width*height*4;if(ftruncate(fd,(off_t)bytes)<0)exit(5);
    void *data=mmap(NULL,bytes,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);if(data==MAP_FAILED)exit(6);
    uint32_t *rgba=data;for(size_t i=0;i<bytes/4;i++){unsigned x=(unsigned)(i%(size_t)width),y=(unsigned)(i/(size_t)width);rgba[i]=test_pattern?((x%251)<<16)|((y%251)<<8)|(y<(unsigned)height/2?0x30:0xd0):0x00271824;}
    struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,(int)bytes);
    struct pixels *p=calloc(1,sizeof(*p));if(!p)exit(7);p->data=data;p->length=bytes;
    p->buffer=wl_shm_pool_create_buffer(pool,0,width,height,width*4,WL_SHM_FORMAT_XRGB8888);
    wl_buffer_add_listener(p->buffer,&buffer_listener,p);wl_shm_pool_destroy(pool);close(fd);
    xdg_surface_set_window_geometry(xdg,0,0,width,height);
    wl_surface_attach(surface,p->buffer,0,0);wl_surface_damage(surface,0,0,width,height);wl_surface_commit(surface);
}
static void configure(void *data,struct xdg_surface *s,uint32_t serial){(void)data;xdg_surface_ack_configure(s,serial);paint();printf("{\"event\":\"configure\",\"width\":%d,\"height\":%d,\"fullscreen\":%s,\"f11_keys\":%u}\n",width,height,fullscreen?"true":"false",f11_keys);fflush(stdout);}
static const struct xdg_surface_listener surface_listener={configure};
static void top_configure(void *data,struct xdg_toplevel *s,int32_t w,int32_t h,struct wl_array *states){(void)data;(void)s;if(w>0)width=w;if(h>0)height=h;fullscreen=false;uint32_t *state;wl_array_for_each(state,states){if(*state==XDG_TOPLEVEL_STATE_FULLSCREEN)fullscreen=true;}}
static void top_close(void *data,struct xdg_toplevel *s){(void)data;(void)s;running=false;}
static const struct xdg_toplevel_listener top_listener={.configure=top_configure,.close=top_close};
static struct wl_seat *clipboard_seat;
static struct wl_data_device_manager *data_manager;
static struct wl_data_device *data_device;
static struct wl_data_offer *selection;
static uint32_t selection_serial;
struct selection_payload {unsigned char *bytes;size_t size;};
static void offer_mime(void *d,struct wl_data_offer *o,const char *mime){(void)d;(void)o;(void)mime;}
static const struct wl_data_offer_listener offer_listener={.offer=offer_mime};
static void data_offer(void *d,struct wl_data_device *v,struct wl_data_offer *o){(void)d;(void)v;wl_data_offer_add_listener(o,&offer_listener,NULL);}
static void data_enter(void*d,struct wl_data_device*v,uint32_t serial,struct wl_surface*surf,wl_fixed_t x,wl_fixed_t y,struct wl_data_offer*o){(void)d;(void)v;(void)serial;(void)surf;(void)x;(void)y;(void)o;}
static void data_leave(void*d,struct wl_data_device*v){(void)d;(void)v;}
static void data_motion(void*d,struct wl_data_device*v,uint32_t time,wl_fixed_t x,wl_fixed_t y){(void)d;(void)v;(void)time;(void)x;(void)y;}
static void data_drop(void*d,struct wl_data_device*v){(void)d;(void)v;}
static void data_selection(void*d,struct wl_data_device*v,struct wl_data_offer*o){(void)d;(void)v;if(selection&&selection!=o)wl_data_offer_destroy(selection);selection=o;}
static const struct wl_data_device_listener data_listener={data_offer,data_enter,data_leave,data_motion,data_drop,data_selection};
static void maybe_data_device(void){if(data_manager&&clipboard_seat&&!data_device){data_device=wl_data_device_manager_get_data_device(data_manager,clipboard_seat);wl_data_device_add_listener(data_device,&data_listener,NULL);}}
static void source_target(void*d,struct wl_data_source*s,const char*m){(void)d;(void)s;(void)m;}
static void source_send(void*d,struct wl_data_source*s,const char*m,int fd){
    (void)s;(void)m;struct selection_payload *p=d;size_t offset=0;
    while(offset<p->size){ssize_t n=write(fd,p->bytes+offset,p->size-offset);if(n>0)offset+=(size_t)n;else if(n<0&&errno==EINTR)continue;else if(n<0&&(errno==EAGAIN||errno==EWOULDBLOCK)){struct pollfd out={.fd=fd,.events=POLLOUT};if(poll(&out,1,5000)>0)continue;else break;}else break;}close(fd);
}
static void source_cancel(void*d,struct wl_data_source*s){struct selection_payload*p=d;free(p->bytes);free(p);wl_data_source_destroy(s);}
static const struct wl_data_source_listener source_listener={.target=source_target,.send=source_send,.cancelled=source_cancel};
static void offer_bytes(unsigned char *bytes,size_t size,const char *mime){
    struct selection_payload*p=calloc(1,sizeof(*p));if(!p)exit(7);p->bytes=bytes;p->size=size;
    struct wl_data_source *source=wl_data_device_manager_create_data_source(data_manager);wl_data_source_add_listener(source,&source_listener,p);wl_data_source_offer(source,mime);wl_data_device_set_selection(data_device,source,selection_serial);
}
static void keymap(void *d,struct wl_keyboard *k,uint32_t f,int fd,uint32_t size){(void)d;(void)k;(void)f;(void)size;close(fd);}
static void enter(void *d,struct wl_keyboard *k,uint32_t serial,struct wl_surface *s,struct wl_array *keys){(void)d;(void)k;selection_serial=serial;(void)s;(void)keys;}
static void leave(void *d,struct wl_keyboard *k,uint32_t serial,struct wl_surface *s){(void)d;(void)k;(void)serial;(void)s;}
static void key(void *d,struct wl_keyboard *k,uint32_t serial,uint32_t time,uint32_t key,uint32_t state){(void)d;(void)k;(void)serial;(void)time;if(log_keys){printf("{\"event\":\"key\",\"code\":%u,\"state\":%u}\n",key,state);fflush(stdout);}if(key==87&&state==WL_KEYBOARD_KEY_STATE_PRESSED){f11_keys++;if(fullscreen)xdg_toplevel_unset_fullscreen(top);else xdg_toplevel_set_fullscreen(top,NULL);printf("{\"event\":\"f11\",\"count\":%u}\n",f11_keys);fflush(stdout);}}
static void modifiers(void *d,struct wl_keyboard *k,uint32_t serial,uint32_t depressed,uint32_t latched,uint32_t locked,uint32_t group){(void)d;(void)k;(void)serial;(void)depressed;(void)latched;(void)locked;(void)group;}
static void repeat(void *d,struct wl_keyboard *k,int32_t rate,int32_t delay){(void)d;(void)k;(void)rate;(void)delay;}
static const struct wl_keyboard_listener keyboard_listener={keymap,enter,leave,key,modifiers,repeat};
#ifdef EMBER_POINTER_TEST
#include "pointer-fixture.h"
#endif
static void capabilities(void *d,struct wl_seat *seat,uint32_t caps){(void)d;
#ifdef EMBER_POINTER_TEST
if(caps&WL_SEAT_CAPABILITY_POINTER)pointer_start(seat);
#endif
if((caps&WL_SEAT_CAPABILITY_KEYBOARD)&&!keyboard){keyboard=wl_seat_get_keyboard(seat);wl_keyboard_add_listener(keyboard,&keyboard_listener,NULL);}}
static void seat_name(void *d,struct wl_seat *s,const char *n){(void)d;(void)s;(void)n;}
static const struct wl_seat_listener seat_listener={capabilities,seat_name};
static void ping(void *data,struct xdg_wm_base *s,uint32_t serial){(void)data;xdg_wm_base_pong(s,serial);}
static const struct xdg_wm_base_listener shell_listener={ping};
static void global(void *data,struct wl_registry *r,uint32_t name,const char *iface,uint32_t version){(void)data;
#ifdef EMBER_POINTER_TEST
    if(!strcmp(iface,"zwp_relative_pointer_manager_v1"))relative_manager=wl_registry_bind(r,name,&zwp_relative_pointer_manager_v1_interface,1);
    if(!strcmp(iface,"zwp_pointer_constraints_v1"))constraints_manager=wl_registry_bind(r,name,&zwp_pointer_constraints_v1_interface,1);
#endif
    if(!strcmp(iface,"wl_compositor"))compositor=wl_registry_bind(r,name,&wl_compositor_interface,4);
    if(!strcmp(iface,"wl_shm"))shm=wl_registry_bind(r,name,&wl_shm_interface,1);
    if(!strcmp(iface,"xdg_wm_base"))shell=wl_registry_bind(r,name,&xdg_wm_base_interface,1);
    if(!strcmp(iface,"wl_seat")){clipboard_seat=wl_registry_bind(r,name,&wl_seat_interface,version<5?version:5);wl_seat_add_listener(clipboard_seat,&seat_listener,NULL);maybe_data_device();}
    if(!strcmp(iface,"wl_data_device_manager")){data_manager=wl_registry_bind(r,name,&wl_data_device_manager_interface,3);maybe_data_device();}
}
static void remove_global(void *d,struct wl_registry *r,uint32_t n){(void)d;(void)r;(void)n;}
static const struct wl_registry_listener registry_listener={global,remove_global};
static void command(char *s){
#ifdef EMBER_POINTER_TEST
    if(pointer_command(s))return;
#endif
    if(!strncmp(s,"clipboard ",10)){if(selection){int fd=open(s+10,O_WRONLY|O_CREAT|O_TRUNC,0600);if(fd>=0){wl_data_offer_receive(selection,"image/png",fd);close(fd);}}}
    else if(!strncmp(s,"clipboard-text ",15)){if(selection){int fd=open(s+15,O_WRONLY|O_CREAT|O_TRUNC,0600);if(fd>=0){wl_data_offer_receive(selection,"text/plain;charset=utf-8",fd);close(fd);}}}
    else if(!strncmp(s,"text ",5)){if(data_manager&&data_device){size_t n=strlen(s+5);unsigned char*bytes=malloc(n);if(!bytes)exit(7);memcpy(bytes,s+5,n);offer_bytes(bytes,n,"text/plain;charset=utf-8");}}
    else if(!strncmp(s,"image ",6)){if(data_manager&&data_device){FILE*f=fopen(s+6,"rb");if(f){fseek(f,0,SEEK_END);long n=ftell(f);rewind(f);if(n<1||n>16*1024*1024)exit(7);unsigned char*bytes=malloc((size_t)n);if(!bytes||fread(bytes,1,(size_t)n,f)!=(size_t)n)exit(7);fclose(f);offer_bytes(bytes,(size_t)n,"image/png");}}}
    else if(!strncmp(s,"title ",6))xdg_toplevel_set_title(top,s+6);
    else if(!strncmp(s,"app-id ",7))xdg_toplevel_set_app_id(top,s+7);
    else if(!strcmp(s,"pattern")){test_pattern=true;paint();}
    else if(!strcmp(s,"fullscreen"))xdg_toplevel_set_fullscreen(top,NULL);
    else if(!strcmp(s,"unfullscreen"))xdg_toplevel_unset_fullscreen(top);
    else if(!strcmp(s,"quit"))running=false;
}
int main(int argc,char **argv){
    const char *app="ember-rule-test",*title="Rule test",*control=NULL;
    for(int i=1;i<argc;i++){if(!strcmp(argv[i],"--app-id")&&i+1<argc)app=argv[++i];else if(!strcmp(argv[i],"--title")&&i+1<argc)title=argv[++i];else if(!strcmp(argv[i],"--control")&&i+1<argc)control=argv[++i];else if(!strcmp(argv[i],"--log-keys"))log_keys=true;else return 9;}
    struct wl_display *display=wl_display_connect(NULL);if(!display)return 2;
    struct wl_registry *registry=wl_display_get_registry(display);wl_registry_add_listener(registry,&registry_listener,NULL);wl_display_roundtrip(display);
    if(!compositor||!shm||!shell)return 3;
    xdg_wm_base_add_listener(shell,&shell_listener,NULL);
    surface=wl_compositor_create_surface(compositor);xdg=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(xdg,&surface_listener,NULL);
    top=xdg_surface_get_toplevel(xdg);xdg_toplevel_add_listener(top,&top_listener,NULL);xdg_toplevel_set_app_id(top,app);xdg_toplevel_set_title(top,title);wl_surface_commit(surface);
    int fd=-1;if(control){if(mkfifo(control,0600)<0&&errno!=EEXIST)return 10;fd=open(control,O_RDWR|O_NONBLOCK);if(fd<0)return 11;}
    char commands[2048];size_t used=0;
    while(running){
        if(wl_display_dispatch_pending(display)<0)break;
        wl_display_flush(display);
        struct pollfd fds[2]={{wl_display_get_fd(display),POLLIN,0},{fd,POLLIN,0}};
        if(poll(fds,2,-1)<0){if(errno==EINTR)continue;break;}
        if(fds[0].revents&(POLLERR|POLLHUP))break;
        if(fds[0].revents&POLLIN){if(wl_display_dispatch(display)<0)break;}
        if(fd>=0&&(fds[1].revents&POLLIN)){
            ssize_t n=read(fd,commands+used,sizeof(commands)-used-1);if(n>0){used+=(size_t)n;commands[used]=0;char *start=commands,*end;
                while((end=strchr(start,'\n'))){*end=0;command(start);start=end+1;}
                used-=((size_t)(start-commands));memmove(commands,start,used);if(used==sizeof(commands)-1)used=0;
            }
        }
    }
    if(fd>=0){close(fd);unlink(control);}xdg_toplevel_destroy(top);xdg_surface_destroy(xdg);wl_surface_destroy(surface);wl_display_flush(display);wl_display_disconnect(display);return 0;
}
