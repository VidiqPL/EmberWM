#define _POSIX_C_SOURCE 200809L
#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <spa/buffer/meta.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <sys/mman.h>
#include <string.h>
struct capture {struct pw_main_loop *loop;struct pw_stream *stream;struct spa_video_info_raw format;unsigned frames;const char *save;};
static void param_changed(void *data,uint32_t id,const struct spa_pod *param){struct capture*c=data;if(id==SPA_PARAM_Format&&param)spa_format_video_raw_parse(param,&c->format);}
static void process(void *data){struct capture*c=data;struct pw_buffer*b=pw_stream_dequeue_buffer(c->stream);if(!b)return;struct spa_buffer*buffer=b->buffer;
 if(buffer->n_datas&&buffer->datas[0].fd>=0&&buffer->datas[0].chunk->size&&c->format.size.width){
  struct spa_data*d=&buffer->datas[0];unsigned width=c->format.size.width,height=c->format.size.height;c->frames++;
  struct spa_meta_videotransform *meta=spa_buffer_find_meta_data(buffer,SPA_META_VideoTransform,sizeof(*meta));unsigned transform=meta?meta->transform:0;
  if(c->frames>=12){
   FILE*f=fopen(c->save,"wb");if(!f)exit(4);fprintf(f,"P6\n%u %u\n255\n",width,height);
   void*read_mapping=mmap(NULL,d->maxsize,PROT_READ,MAP_SHARED,d->fd,d->mapoffset);if(read_mapping==MAP_FAILED)exit(34);uint8_t *base=(uint8_t*)read_mapping+d->chunk->offset;int32_t stride=d->chunk->stride?d->chunk->stride:(int32_t)width*4;
   for(unsigned y=0;y<height;y++)for(unsigned x=0;x<width;x++){
    unsigned sy=transform==6?height-1-y:y;ptrdiff_t offset=(ptrdiff_t)d->chunk->offset+(ptrdiff_t)sy*stride+x*4;if(offset<0||(size_t)offset+4>d->maxsize){fprintf(stderr,"bad pixel x=%u y=%u sy=%u offset=%td\n",x,y,sy,offset);exit(33);}uint8_t*p=base+(ptrdiff_t)sy*stride+x*4;uint8_t rgb[3];
    switch(c->format.format){case SPA_VIDEO_FORMAT_BGRx:case SPA_VIDEO_FORMAT_BGRA:rgb[0]=p[2];rgb[1]=p[1];rgb[2]=p[0];break;default:rgb[0]=p[0];rgb[1]=p[1];rgb[2]=p[2];break;}fwrite(rgb,1,3,f);
   }fclose(f);munmap(read_mapping,d->maxsize);printf("{\"frames\":%u,\"width\":%u,\"height\":%u,\"format\":%u,\"transform\":%u}\n",c->frames,width,height,c->format.format,transform);fflush(stdout);pw_main_loop_quit(c->loop);
  }
 }
 pw_stream_queue_buffer(c->stream,b);
}
static void state_changed(void *data,enum pw_stream_state old,enum pw_stream_state state,const char*error){(void)old;if(state==PW_STREAM_STATE_ERROR){fprintf(stderr,"PipeWire stream: %s\n",error);pw_main_loop_quit(((struct capture*)data)->loop);}}
static const struct pw_stream_events events={PW_VERSION_STREAM_EVENTS,.state_changed=state_changed,.param_changed=param_changed,.process=process};
int main(int argc,char**argv){if(argc!=4)return 2;int fd=atoi(argv[1]);uint32_t node=(uint32_t)strtoul(argv[2],NULL,10);struct capture c={.save=argv[3]};pw_init(NULL,NULL);c.loop=pw_main_loop_new(NULL);struct pw_context*context=pw_context_new(pw_main_loop_get_loop(c.loop),NULL,0);struct pw_core*core=pw_context_connect_fd(context,fd,NULL,0);if(!core)return 3;
 c.stream=pw_stream_new(core,"Ember portal video test",pw_properties_new(PW_KEY_MEDIA_TYPE,"Video",PW_KEY_MEDIA_CATEGORY,"Capture",PW_KEY_MEDIA_ROLE,"Screen",NULL));struct spa_hook listener;pw_stream_add_listener(c.stream,&listener,&events,&c);
 uint8_t storage[1024];struct spa_pod_builder builder=SPA_POD_BUILDER_INIT(storage,sizeof(storage));const struct spa_pod*params[2];params[0]=spa_pod_builder_add_object(&builder,SPA_TYPE_OBJECT_Format,SPA_PARAM_EnumFormat,SPA_FORMAT_mediaType,SPA_POD_Id(SPA_MEDIA_TYPE_video),SPA_FORMAT_mediaSubtype,SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),SPA_FORMAT_VIDEO_format,SPA_POD_CHOICE_ENUM_Id(4,SPA_VIDEO_FORMAT_BGRA,SPA_VIDEO_FORMAT_BGRx,SPA_VIDEO_FORMAT_RGBA,SPA_VIDEO_FORMAT_RGBx));
 params[1]=spa_pod_builder_add_object(&builder,SPA_TYPE_OBJECT_ParamMeta,SPA_PARAM_Meta,SPA_PARAM_META_type,SPA_POD_Id(SPA_META_VideoTransform),SPA_PARAM_META_size,SPA_POD_Int(sizeof(struct spa_meta_videotransform)));
 if(pw_stream_connect(c.stream,PW_DIRECTION_INPUT,node,PW_STREAM_FLAG_AUTOCONNECT,params,2)<0)return 5;
 pw_main_loop_run(c.loop);pw_stream_destroy(c.stream);pw_core_disconnect(core);pw_context_destroy(context);pw_main_loop_destroy(c.loop);pw_deinit();return c.frames>=12?0:6;
}
