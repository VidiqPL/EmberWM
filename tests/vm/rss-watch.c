/* Sample one test process, never other user processes. RSS peak for screenshot benchmarks. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
static long rss(const char *path){
 FILE*f=fopen(path,"r");if(!f)return -1;char line[256];long value=-1;
 while(fgets(line,sizeof line,f))if(sscanf(line,"VmRSS: %ld kB",&value)==1)break;
 fclose(f);return value;
}
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (double)t.tv_sec+t.tv_nsec/1e9;}
int main(int argc,char**argv){
 if(argc!=3)return 2;
 char path[64];snprintf(path,sizeof path,"/proc/%ld/status",strtol(argv[1],NULL,10));
 long initial=rss(path),peak=initial;double end=now()+strtod(argv[2],NULL);unsigned samples=0;
 struct timespec pause={.tv_sec=0,.tv_nsec=1000000};
 while(now()<end){long value=rss(path);if(value>peak)peak=value;samples++;nanosleep(&pause,NULL);}
 printf("{\"rss_start_kib\":%ld,\"rss_peak_kib\":%ld,\"rss_peak_delta_kib\":%ld,\"samples\":%u}\n",initial,peak,peak-initial,samples);
 return initial<0?3:0;
}
