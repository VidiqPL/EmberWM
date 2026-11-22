#define _GNU_SOURCE
/* X11 listeners are allocated by Smithay. Wait without starting the X server
 * until a client connects, then preserve all inherited descriptors across exec. */
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <unistd.h>
int main(int argc,char **argv) {
    const char *real=getenv("EMBER_REAL_XWAYLAND"),*wayland=getenv("WAYLAND_SOCKET");
    const char *auth=getenv("EMBER_XWAYLAND_AUTH"),*pidfile=getenv("EMBER_XWAYLAND_PID_FILE");
    if(!real||!wayland||!auth)return 2;
    if(prctl(PR_SET_PDEATHSIG,SIGTERM)||getppid()==1)return 2;
    if(pidfile){FILE *f=fopen(pidfile,"w");if(f){fprintf(f,"%ld\n",(long)getpid());fclose(f);}}
    struct pollfd fds[9];int count=1;
    fds[0]=(struct pollfd){.fd=atoi(wayland),.events=0};
    for(int i=1;i+1<argc;i++)if(!strcmp(argv[i],"-listenfd")) {
        if(count>=9)return 2;
        fds[count++]=(struct pollfd){.fd=atoi(argv[++i]),.events=POLLIN};
    }
    if(count<2)return 2;
    for(;;) {
        if(poll(fds,(nfds_t)count,-1)<0)return 2;
        if(fds[0].revents&(POLLHUP|POLLERR|POLLNVAL))return 0;
        int ready=0;for(int i=1;i<count;i++)if(fds[i].revents&POLLIN)ready=1;
        if(ready)break;
    }
    char **args=calloc((size_t)argc+6,sizeof(*args));if(!args)return 2;
    args[0]=(char *)real;
    for(int i=1;i<argc;i++)args[i]=argv[i];
    args[argc]="-auth";args[argc+1]=(char *)auth;
    args[argc+2]="-nolisten";args[argc+3]="tcp";
    const char *search=getenv("EMBER_XWAYLAND_REAL_PATH");
    if(!search||setenv("PATH",search,1))return 2;
    execv(real,args);perror("Ember Xwayland exec");return 127;
}
