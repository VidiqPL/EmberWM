#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "xdg-shell-client.h"
#include "presentation-time-client.h"
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *shell;
static struct wl_surface *surface;
static struct wp_presentation *presentation;
static unsigned feedback_count=0, discarded_count=0, invalid_feedback=0;
static uint32_t clock_id=UINT32_MAX, min_refresh=UINT32_MAX, max_refresh=0;
static uint64_t last_presented=0, last_sequence=0;
static double last_frame=0,max_frame_gap=0;
static void clock_event(void *data, struct wp_presentation *p, uint32_t id){(void)data;(void)p;clock_id=id;}
static const struct wp_presentation_listener presentation_listener={clock_event};
static void sync_output(void *data,struct wp_presentation_feedback *f,struct wl_output *output){(void)data;(void)f;(void)output;}
static void presented(void *data,struct wp_presentation_feedback *f,uint32_t hi,uint32_t lo,uint32_t ns,uint32_t refresh,uint32_t seq_hi,uint32_t seq_lo,uint32_t flags){
    (void)data;
    uint64_t t=(((uint64_t)hi<<32)|lo)*1000000000ULL+ns, seq=((uint64_t)seq_hi<<32)|seq_lo;
    if(ns>=1000000000U || (last_presented && t<last_presented) ||
       (last_sequence && seq<last_sequence) || !(flags&WP_PRESENTATION_FEEDBACK_KIND_VSYNC))invalid_feedback++;
    last_presented=t;last_sequence=seq;feedback_count++;
    if(refresh<min_refresh)min_refresh=refresh;
    if(refresh>max_refresh)max_refresh=refresh;
    wp_presentation_feedback_destroy(f);
}
static void discarded(void *data,struct wp_presentation_feedback *f){(void)data;discarded_count++;wp_presentation_feedback_destroy(f);}
static const struct wp_presentation_feedback_listener feedback_listener={sync_output,presented,discarded};
static struct wl_buffer *buffer;
static uint32_t *pixels;
static bool running=true,started=false,configured=false;
static unsigned frames=0;
static double began;
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
static void paint(void);
static void frame_done(void *data,struct wl_callback *callback,uint32_t time){
    (void)data;(void)time;wl_callback_destroy(callback);
    if(!started){started=true;began=now();frames=0;}else frames++;
    double stamp=now(), elapsed=stamp-began;
    if(last_frame && stamp-last_frame>max_frame_gap)max_frame_gap=stamp-last_frame;
    last_frame=stamp;
    if(elapsed>=3.0){
        printf("{\"frames\":%u,\"elapsed_seconds\":%.6f,\"fps\":%.3f,\"max_frame_gap_ms\":%.3f,\"presentation_available\":%s,\"clock_id\":%u,\"presented\":%u,\"discarded\":%u,\"invalid_feedback\":%u,\"min_refresh_ns\":%u,\"max_refresh_ns\":%u}\n",frames,elapsed,frames/elapsed,max_frame_gap*1000,presentation?"true":"false",clock_id,feedback_count,discarded_count,invalid_feedback,feedback_count?min_refresh:0,max_refresh);
        running=false;return;
    }
    paint();
}
static const struct wl_callback_listener frame_listener={frame_done};
static void paint(void){
    pixels[0]^=0x00ffffff;
    wl_surface_attach(surface,buffer,0,0);wl_surface_damage(surface,0,0,64,64);
    struct wl_callback *cb=wl_surface_frame(surface);wl_callback_add_listener(cb,&frame_listener,NULL);
    if(presentation){struct wp_presentation_feedback *f=wp_presentation_feedback(presentation,surface);wp_presentation_feedback_add_listener(f,&feedback_listener,NULL);}
    wl_surface_commit(surface);
}
static void configure(void *data,struct xdg_surface *xdg,uint32_t serial){
    (void)data;xdg_surface_ack_configure(xdg,serial);if(!configured){configured=true;paint();}
}
static const struct xdg_surface_listener configure_listener={configure};
static void top_configure(void *data,struct xdg_toplevel *top,int32_t w,int32_t h,struct wl_array *states){(void)data;(void)top;(void)w;(void)h;(void)states;}
static void top_close(void *data,struct xdg_toplevel *top){(void)data;(void)top;running=false;}
static const struct xdg_toplevel_listener top_listener={.configure=top_configure,.close=top_close};
static void ping(void *data,struct xdg_wm_base *base,uint32_t serial){(void)data;xdg_wm_base_pong(base,serial);}
static const struct xdg_wm_base_listener shell_listener={ping};
static void global(void *data,struct wl_registry *registry,uint32_t name,const char *interface,uint32_t version){
    (void)data;(void)version;
    if(!__builtin_strcmp(interface,"wp_presentation")){
        presentation=wl_registry_bind(registry,name,&wp_presentation_interface,1);
        wp_presentation_add_listener(presentation,&presentation_listener,NULL);
    }
    if(!__builtin_strcmp(interface,"wl_compositor"))compositor=wl_registry_bind(registry,name,&wl_compositor_interface,4);
    if(!__builtin_strcmp(interface,"wl_shm"))shm=wl_registry_bind(registry,name,&wl_shm_interface,1);
    if(!__builtin_strcmp(interface,"xdg_wm_base"))shell=wl_registry_bind(registry,name,&xdg_wm_base_interface,1);
}
static void remove_global(void *data,struct wl_registry *r,uint32_t n){(void)data;(void)r;(void)n;}
static const struct wl_registry_listener registry_listener={global,remove_global};
int main(void){
    struct wl_display *display=wl_display_connect(NULL);if(!display)return 2;
    struct wl_registry *registry=wl_display_get_registry(display);wl_registry_add_listener(registry,&registry_listener,NULL);
    wl_display_roundtrip(display);if(!compositor||!shm||!shell)return 3;
    xdg_wm_base_add_listener(shell,&shell_listener,NULL);
    char name[]="/tmp/ember-frame-XXXXXX";int fd=mkstemp(name);if(fd<0)return 4;unlink(name);
    if(ftruncate(fd,64*64*4)<0)return 5;
    pixels=mmap(NULL,64*64*4,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);if(pixels==MAP_FAILED)return 6;
    for(int i=0;i<64*64;i++)pixels[i]=0x0035d0a0;
    struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,64*64*4);
    buffer=wl_shm_pool_create_buffer(pool,0,64,64,64*4,WL_SHM_FORMAT_XRGB8888);wl_shm_pool_destroy(pool);close(fd);
    surface=wl_compositor_create_surface(compositor);
    struct xdg_surface *xdg=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(xdg,&configure_listener,NULL);
    struct xdg_toplevel *top=xdg_surface_get_toplevel(xdg);xdg_toplevel_add_listener(top,&top_listener,NULL);xdg_toplevel_set_app_id(top,"ember-frame-benchmark");
    xdg_toplevel_set_title(top,"Ember frame callback test");wl_surface_commit(surface);
    while(running && wl_display_dispatch(display)>=0){}
    xdg_toplevel_destroy(top);xdg_surface_destroy(xdg);wl_surface_destroy(surface);wl_buffer_destroy(buffer);
    wl_display_disconnect(display);munmap(pixels,64*64*4);return frames?0:7;
}
