#define _POSIX_C_SOURCE 200809L
#include <wayland-client.h>
#include "screencopy-client.h"
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
static struct wl_shm *shm;
static struct wl_output *output;
static struct zwlr_screencopy_manager_v1 *manager;
static struct wl_buffer *buffer;
static uint8_t *pixels;
static size_t bytes;
static unsigned width,height,stride,flags,frames;
static int output_transform;
static uint8_t samples[6];
static bool done,failed,use_damage,bad_buffer;
static const char *save_path;
static void offered(void *data,struct zwlr_screencopy_frame_v1 *frame,uint32_t format,uint32_t w,uint32_t h,uint32_t s){
 (void)data;(void)frame; width=w;height=h;stride=s;bytes=(size_t)h*s;
 if(format!=WL_SHM_FORMAT_ARGB8888||bytes>268435456)exit(7);
 char path[]="/tmp/ember-copy-XXXXXX";int fd=mkstemp(path);if(fd<0)exit(8);unlink(path);if(ftruncate(fd,(off_t)bytes)<0)exit(9);
 pixels=mmap(NULL,bytes,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);if(pixels==MAP_FAILED)exit(10);
 struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,(int)bytes);buffer=wl_shm_pool_create_buffer(pool,0,(int)w-(bad_buffer?1:0),(int)h,(int)s,format);wl_shm_pool_destroy(pool);close(fd);
}
static void copied_flags(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t v){(void)d;(void)f;flags=v;}
static void ready(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t hi,uint32_t lo,uint32_t ns){(void)d;(void)f;(void)hi;(void)lo;(void)ns;if(pixels&&width>201&&height>220){uint8_t*a=pixels+(size_t)120*stride+100*4;uint8_t*b=pixels+(size_t)(height-100)*stride+200*4;samples[0]=a[2];samples[1]=a[1];samples[2]=a[0];samples[3]=b[2];samples[4]=b[1];samples[5]=b[0];}frames++;done=true;}
static void fail(void*d,struct zwlr_screencopy_frame_v1*f){(void)d;(void)f;failed=true;done=true;}
static void damage(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t x,uint32_t y,uint32_t w,uint32_t h){(void)d;(void)f;(void)x;(void)y;(void)w;(void)h;}
static void dmabuf(void*d,struct zwlr_screencopy_frame_v1*f,uint32_t fmt,uint32_t w,uint32_t h){(void)d;(void)f;(void)fmt;(void)w;(void)h;}
static void buffer_done(void*d,struct zwlr_screencopy_frame_v1*f){(void)d;if(use_damage)zwlr_screencopy_frame_v1_copy_with_damage(f,buffer);else zwlr_screencopy_frame_v1_copy(f,buffer);}
static const struct zwlr_screencopy_frame_v1_listener frame_listener={offered,copied_flags,ready,fail,damage,dmabuf,buffer_done};
static void geometry(void*d,struct wl_output*o,int32_t x,int32_t y,int32_t w,int32_t h,int32_t sub,const char*make,const char*model,int32_t tr){(void)d;(void)o;(void)x;(void)y;(void)w;(void)h;(void)sub;(void)make;(void)model;output_transform=tr;}
static void mode(void*d,struct wl_output*o,uint32_t flags1,int32_t w,int32_t h,int32_t hz){(void)d;(void)o;(void)flags1;(void)w;(void)h;(void)hz;}
static void output_done(void*d,struct wl_output*o){(void)d;(void)o;}
static void scale(void*d,struct wl_output*o,int32_t factor){(void)d;(void)o;(void)factor;}
static const struct wl_output_listener output_listener={.geometry=geometry,.mode=mode,.done=output_done,.scale=scale};
static void global(void*d,struct wl_registry*r,uint32_t name,const char*iface,uint32_t version){(void)d;
 if(!strcmp(iface,"wl_shm"))shm=wl_registry_bind(r,name,&wl_shm_interface,1);
 if(!strcmp(iface,"wl_output")&&!output){output=wl_registry_bind(r,name,&wl_output_interface,version<3?version:3);wl_output_add_listener(output,&output_listener,NULL);}
 if(!strcmp(iface,"zwlr_screencopy_manager_v1"))manager=wl_registry_bind(r,name,&zwlr_screencopy_manager_v1_interface,3);
}
static void removed(void*d,struct wl_registry*r,uint32_t n){(void)d;(void)r;(void)n;}
static const struct wl_registry_listener registry_listener={global,removed};
int main(int argc,char **argv){
 int cursor=0,count=1;bool region=false;int x=0,y=0,w=0,h=0;
 for(int i=1;i<argc;i++){
  if(!strcmp(argv[i],"--cursor"))cursor=1;
  else if(!strcmp(argv[i],"--damage"))use_damage=true;
  else if(!strcmp(argv[i],"--bad-buffer"))bad_buffer=true;
  else if(!strcmp(argv[i],"--frames")&&i+1<argc)count=atoi(argv[++i]);
  else if(!strcmp(argv[i],"--save")&&i+1<argc)save_path=argv[++i];
  else if(!strcmp(argv[i],"--region")&&i+4<argc){region=true;x=atoi(argv[++i]);y=atoi(argv[++i]);w=atoi(argv[++i]);h=atoi(argv[++i]);}
  else return 2;
 }
 struct wl_display *display=wl_display_connect(NULL);if(!display)return 3;
 struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&registry_listener,NULL);wl_display_roundtrip(display);if(!shm||!output||!manager)return 4;
 for(int i=0;i<count;i++){
  done=false;failed=false;struct zwlr_screencopy_frame_v1 *frame=region?zwlr_screencopy_manager_v1_capture_output_region(manager,cursor,output,x,y,w,h):zwlr_screencopy_manager_v1_capture_output(manager,cursor,output);
  zwlr_screencopy_frame_v1_add_listener(frame,&frame_listener,NULL);while(!done){if(wl_display_dispatch(display)<0)return 11;}
  if(!failed&&save_path&&i==count-1){FILE*f=fopen(save_path,"wb");if(!f)return 12;fprintf(f,"P6\n%u %u\n255\n",width,height);for(unsigned y1=0;y1<height;y1++){unsigned row=(flags&1)?height-1-y1:y1;if(output_transform==6)row=height-1-row;for(unsigned x1=0;x1<width;x1++){uint8_t*p=pixels+(size_t)row*stride+x1*4;uint8_t rgb[]={p[2],p[1],p[0]};fwrite(rgb,1,3,f);}}fclose(f);}
  zwlr_screencopy_frame_v1_destroy(frame);if(buffer){wl_buffer_destroy(buffer);buffer=NULL;}if(pixels){munmap(pixels,bytes);pixels=NULL;}if(failed)break;
 }
 printf("{\"frames\":%u,\"width\":%u,\"height\":%u,\"failed\":%s,\"samples\":[[%u,%u,%u],[%u,%u,%u]]}\n",frames,width,height,failed?"true":"false",samples[0],samples[1],samples[2],samples[3],samples[4],samples[5]);wl_display_disconnect(display);return failed?5:0;
}
