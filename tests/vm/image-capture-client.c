#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "foreign-toplevel-client.h"
#include "image-source-client.h"
#include "image-copy-client.h"
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
static struct wl_shm *shm;
static struct wl_output *output;
static struct ext_foreign_toplevel_list_v1 *list;
static struct ext_output_image_capture_source_manager_v1 *output_manager;
static struct ext_foreign_toplevel_image_capture_source_manager_v1 *window_manager;
static struct ext_image_copy_capture_manager_v1 *manager;
struct top {struct ext_foreign_toplevel_handle_v1 *handle;char app[256],id[64];};
static struct top tops[64];static unsigned ntops;
static unsigned width,height,frames,reason;static bool negotiated,done,failed,stopped,bad_buffer,shm_format;
static uint32_t transform;
static void title(void*d,struct ext_foreign_toplevel_handle_v1*t,const char*v){(void)d;(void)t;(void)v;}
static void app(void*d,struct ext_foreign_toplevel_handle_v1*t,const char*v){(void)t;snprintf(((struct top*)d)->app,256,"%s",v);}
static void id(void*d,struct ext_foreign_toplevel_handle_v1*t,const char*v){(void)t;snprintf(((struct top*)d)->id,64,"%s",v);}
static void closed(void*d,struct ext_foreign_toplevel_handle_v1*t){(void)t;((struct top*)d)->handle=NULL;}
static void top_done(void*d,struct ext_foreign_toplevel_handle_v1*t){(void)d;(void)t;}
static const struct ext_foreign_toplevel_handle_v1_listener top_listener={.title=title,.app_id=app,.identifier=id,.closed=closed,.done=top_done};
static void toplevel(void*d,struct ext_foreign_toplevel_list_v1*l,struct ext_foreign_toplevel_handle_v1*t){(void)d;(void)l;if(ntops>=64)exit(20);struct top*p=&tops[ntops++];p->handle=t;ext_foreign_toplevel_handle_v1_add_listener(t,&top_listener,p);}
static void finished(void*d,struct ext_foreign_toplevel_list_v1*l){(void)d;(void)l;}
static const struct ext_foreign_toplevel_list_v1_listener list_listener={toplevel,finished};
static void global(void*d,struct wl_registry*r,uint32_t n,const char*i,uint32_t v){(void)d;(void)v;
#define BIND(iface,var,type) if(!strcmp(i,iface))var=wl_registry_bind(r,n,&type##_interface,1)
 BIND("wl_shm",shm,wl_shm);BIND("wl_output",output,wl_output);
 BIND("ext_output_image_capture_source_manager_v1",output_manager,ext_output_image_capture_source_manager_v1);
 BIND("ext_foreign_toplevel_image_capture_source_manager_v1",window_manager,ext_foreign_toplevel_image_capture_source_manager_v1);
 BIND("ext_image_copy_capture_manager_v1",manager,ext_image_copy_capture_manager_v1);
 if(!strcmp(i,"ext_foreign_toplevel_list_v1")){list=wl_registry_bind(r,n,&ext_foreign_toplevel_list_v1_interface,1);ext_foreign_toplevel_list_v1_add_listener(list,&list_listener,NULL);}
}
static void removed(void*d,struct wl_registry*r,uint32_t n){(void)d;(void)r;(void)n;}
static const struct wl_registry_listener registry_listener={global,removed};
static void size(void*d,struct ext_image_copy_capture_session_v1*s,uint32_t w,uint32_t h){(void)d;(void)s;width=w;height=h;}
static void format(void*d,struct ext_image_copy_capture_session_v1*s,uint32_t f){(void)d;(void)s;if(f==WL_SHM_FORMAT_ARGB8888)shm_format=true;}
static void device(void*d,struct ext_image_copy_capture_session_v1*s,struct wl_array*a){(void)d;(void)s;(void)a;}
static void dma_format(void*d,struct ext_image_copy_capture_session_v1*s,uint32_t f,struct wl_array*a){(void)d;(void)s;(void)f;(void)a;}
static void session_done(void*d,struct ext_image_copy_capture_session_v1*s){(void)d;(void)s;negotiated=true;}
static void stop(void*d,struct ext_image_copy_capture_session_v1*s){(void)d;(void)s;stopped=true;done=true;}
static const struct ext_image_copy_capture_session_v1_listener session_listener={size,format,device,dma_format,session_done,stop};
static void frame_transform(void*d,struct ext_image_copy_capture_frame_v1*f,uint32_t t){(void)d;(void)f;transform=t;}
static void damage(void*d,struct ext_image_copy_capture_frame_v1*f,int32_t x,int32_t y,int32_t w,int32_t h){(void)d;(void)f;(void)x;(void)y;(void)w;(void)h;}
static void time_presented(void*d,struct ext_image_copy_capture_frame_v1*f,uint32_t hi,uint32_t lo,uint32_t ns){(void)d;(void)f;(void)hi;(void)lo;(void)ns;}
static void ready(void*d,struct ext_image_copy_capture_frame_v1*f){(void)d;(void)f;frames++;done=true;}
static void fail(void*d,struct ext_image_copy_capture_frame_v1*f,uint32_t r){(void)d;(void)f;reason=r;failed=true;done=true;}
static const struct ext_image_copy_capture_frame_v1_listener frame_listener={frame_transform,damage,time_presented,ready,fail};
int main(int argc,char**argv){
 const char *wanted=NULL,*save=NULL;int count=1,cursor=0;
 for(int i=1;i<argc;i++){if(!strcmp(argv[i],"--window")&&i+1<argc)wanted=argv[++i];else if(!strcmp(argv[i],"--save")&&i+1<argc)save=argv[++i];else if(!strcmp(argv[i],"--frames")&&i+1<argc)count=atoi(argv[++i]);else if(!strcmp(argv[i],"--bad-buffer"))bad_buffer=true;else if(!strcmp(argv[i],"--cursor"))cursor=1;else return 2;}
 struct wl_display*d=wl_display_connect(NULL);if(!d)return 3;struct wl_registry*r=wl_display_get_registry(d);wl_registry_add_listener(r,&registry_listener,NULL);
 wl_display_roundtrip(d);wl_display_roundtrip(d);wl_display_roundtrip(d);
 if(!shm||!manager||!output_manager||!window_manager||!list)return 4;
 struct ext_image_capture_source_v1 *source=NULL;
 if(wanted){for(unsigned i=0;i<ntops;i++)if(tops[i].handle&&!strcmp(tops[i].app,wanted)){source=ext_foreign_toplevel_image_capture_source_manager_v1_create_source(window_manager,tops[i].handle);break;}}else source=ext_output_image_capture_source_manager_v1_create_source(output_manager,output);
 if(!source)return 6;
 struct ext_image_copy_capture_session_v1*s=ext_image_copy_capture_manager_v1_create_session(manager,source,cursor);ext_image_copy_capture_session_v1_add_listener(s,&session_listener,NULL);ext_image_capture_source_v1_destroy(source);
 while(!negotiated&&!stopped)if(wl_display_dispatch(d)<0)return 11;
 if(stopped||!shm_format||!width||!height||width>8192||height>8192)return 7;
 for(int i=0;i<count&&!stopped;i++){
  size_t bytes=(size_t)width*height*4;char name[]="/tmp/ember-ext-capture-XXXXXX";int fd=mkstemp(name);if(fd<0)return 8;unlink(name);if(ftruncate(fd,(off_t)bytes)<0)return 9;
  uint8_t*p=mmap(NULL,bytes,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);if(p==MAP_FAILED)return 10;
  struct wl_shm_pool*pool=wl_shm_create_pool(shm,fd,(int)bytes);struct wl_buffer*b=wl_shm_pool_create_buffer(pool,0,(int)width-(bad_buffer?1:0),(int)height,(int)width*4,WL_SHM_FORMAT_ARGB8888);wl_shm_pool_destroy(pool);close(fd);
  struct ext_image_copy_capture_frame_v1*f=ext_image_copy_capture_session_v1_create_frame(s);ext_image_copy_capture_frame_v1_add_listener(f,&frame_listener,NULL);done=false;failed=false;
  ext_image_copy_capture_frame_v1_attach_buffer(f,b);ext_image_copy_capture_frame_v1_damage_buffer(f,0,0,(int)width,(int)height);ext_image_copy_capture_frame_v1_capture(f);
  while(!done)if(wl_display_dispatch(d)<0)return 11;
  if(!failed&&!stopped&&save&&i==count-1){FILE*file=fopen(save,"wb");if(!file)return 12;fprintf(file,"P6\n%u %u\n255\n",width,height);for(unsigned y=0;y<height;y++)for(unsigned x=0;x<width;x++){uint8_t*px=p+((size_t)y*width+x)*4;uint8_t rgb[]={px[2],px[1],px[0]};fwrite(rgb,1,3,file);}fclose(file);}
  ext_image_copy_capture_frame_v1_destroy(f);wl_buffer_destroy(b);munmap(p,bytes);if(failed)break;
 }
 printf("{\"frames\":%u,\"width\":%u,\"height\":%u,\"transform\":%u,\"failed\":%s,\"reason\":%u,\"stopped\":%s}\n",frames,width,height,transform,failed?"true":"false",reason,stopped?"true":"false");
 ext_image_copy_capture_session_v1_destroy(s);wl_display_flush(d);wl_display_disconnect(d);return failed?5:0;
}
