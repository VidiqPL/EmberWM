static struct wl_seat *pointer_seat;
static uint32_t pointer_button_serial;
/* Included only by the synthetic WM fixture: never logs physical host input. */
#include "relative-pointer-client.h"
#include "pointer-constraints-client.h"
static struct zwp_relative_pointer_manager_v1 *relative_manager;
static struct zwp_pointer_constraints_v1 *constraints_manager;
static struct zwp_relative_pointer_v1 *relative;
static struct zwp_locked_pointer_v1 *locked;
static struct zwp_confined_pointer_v1 *confined;
static struct wl_pointer *pointer;
static void motion_relative(void*d,struct zwp_relative_pointer_v1*p,uint32_t hi,uint32_t lo,wl_fixed_t x,wl_fixed_t y,wl_fixed_t ux,wl_fixed_t uy){(void)d;(void)p;(void)hi;(void)lo;printf("{\"event\":\"relative\",\"dx\":%.3f,\"dy\":%.3f,\"raw_dx\":%.3f,\"raw_dy\":%.3f}\n",wl_fixed_to_double(x),wl_fixed_to_double(y),wl_fixed_to_double(ux),wl_fixed_to_double(uy));fflush(stdout);}
static const struct zwp_relative_pointer_v1_listener relative_listener={motion_relative};
static void p_enter(void*d,struct wl_pointer*p,uint32_t serial,struct wl_surface*s,wl_fixed_t x,wl_fixed_t y){(void)d;(void)s;wl_pointer_set_cursor(p,serial,NULL,0,0);printf("{\"event\":\"pointer-enter\",\"x\":%.3f,\"y\":%.3f}\n",wl_fixed_to_double(x),wl_fixed_to_double(y));fflush(stdout);}
static void p_leave(void*d,struct wl_pointer*p,uint32_t serial,struct wl_surface*s){(void)d;(void)p;(void)serial;(void)s;puts("{\"event\":\"pointer-leave\"}");fflush(stdout);}
static void p_motion(void*d,struct wl_pointer*p,uint32_t time,wl_fixed_t x,wl_fixed_t y){(void)d;(void)p;(void)time;printf("{\"event\":\"pointer-motion\",\"x\":%.3f,\"y\":%.3f}\n",wl_fixed_to_double(x),wl_fixed_to_double(y));fflush(stdout);}
static void p_button(void*d,struct wl_pointer*p,uint32_t s,uint32_t t,uint32_t b,uint32_t state){if(state==WL_POINTER_BUTTON_STATE_PRESSED)pointer_button_serial=s;printf("{\"event\":\"pointer-button\",\"button\":%u,\"state\":%u}\n",b,state);fflush(stdout);(void)d;(void)p;(void)s;(void)t;(void)b;(void)state;}
static void p_axis(void*d,struct wl_pointer*p,uint32_t t,uint32_t axis,wl_fixed_t v){(void)d;(void)p;(void)t;printf("{\"event\":\"pointer-axis\",\"axis\":%u,\"value\":%.3f}\n",axis,wl_fixed_to_double(v));fflush(stdout);}
static void p_frame(void*d,struct wl_pointer*p){(void)d;(void)p;}
static void p_axis_source(void*d,struct wl_pointer*p,uint32_t source){(void)d;(void)p;(void)source;}
static void p_axis_stop(void*d,struct wl_pointer*p,uint32_t t,uint32_t axis){(void)d;(void)p;(void)t;(void)axis;}
static void p_axis_discrete(void*d,struct wl_pointer*p,uint32_t axis,int32_t value){(void)d;(void)p;(void)axis;(void)value;}
static const struct wl_pointer_listener pointer_listener={.enter=p_enter,.leave=p_leave,.motion=p_motion,.button=p_button,.axis=p_axis,.frame=p_frame,.axis_source=p_axis_source,.axis_stop=p_axis_stop,.axis_discrete=p_axis_discrete};
static void on_locked(void*d,struct zwp_locked_pointer_v1*p){(void)d;(void)p;puts("{\"event\":\"locked\"}");fflush(stdout);}
static void on_unlocked(void*d,struct zwp_locked_pointer_v1*p){(void)d;(void)p;puts("{\"event\":\"unlocked\"}");fflush(stdout);}
static const struct zwp_locked_pointer_v1_listener locked_listener={on_locked,on_unlocked};
static void on_confined(void*d,struct zwp_confined_pointer_v1*p){(void)d;(void)p;puts("{\"event\":\"confined\"}");fflush(stdout);}
static void on_unconfined(void*d,struct zwp_confined_pointer_v1*p){(void)d;(void)p;puts("{\"event\":\"unconfined\"}");fflush(stdout);}
static const struct zwp_confined_pointer_v1_listener confined_listener={on_confined,on_unconfined};
static void pointer_start(struct wl_seat*seat){pointer_seat=seat;if(pointer)return;pointer=wl_seat_get_pointer(seat);wl_pointer_add_listener(pointer,&pointer_listener,NULL);if(relative_manager){relative=zwp_relative_pointer_manager_v1_get_relative_pointer(relative_manager,pointer);zwp_relative_pointer_v1_add_listener(relative,&relative_listener,NULL);}}
static bool pointer_command(char*s){
 if(!strcmp(s,"move")){if(pointer_seat&&pointer_button_serial)xdg_toplevel_move(top,pointer_seat,pointer_button_serial);return true;}
 if(!strcmp(s,"resize")){if(pointer_seat&&pointer_button_serial)xdg_toplevel_resize(top,pointer_seat,pointer_button_serial,XDG_TOPLEVEL_RESIZE_EDGE_BOTTOM_RIGHT);return true;}

 if(!strcmp(s,"lock")||!strcmp(s,"lock-once")){if(!locked&&constraints_manager&&pointer){locked=zwp_pointer_constraints_v1_lock_pointer(constraints_manager,surface,pointer,NULL,!strcmp(s,"lock-once")?ZWP_POINTER_CONSTRAINTS_V1_LIFETIME_ONESHOT:ZWP_POINTER_CONSTRAINTS_V1_LIFETIME_PERSISTENT);zwp_locked_pointer_v1_add_listener(locked,&locked_listener,NULL);}return true;}
 if(!strcmp(s,"unlock")){if(locked){zwp_locked_pointer_v1_destroy(locked);locked=NULL;}return true;}
 if(!strncmp(s,"hint ",5)){int x,y;if(locked&&sscanf(s+5,"%d %d",&x,&y)==2){zwp_locked_pointer_v1_set_cursor_position_hint(locked,wl_fixed_from_int(x),wl_fixed_from_int(y));wl_surface_commit(surface);}return true;}
 if(!strcmp(s,"confine")){if(!confined&&constraints_manager&&pointer){struct wl_region*r=wl_compositor_create_region(compositor);wl_region_add(r,50,50,200,160);confined=zwp_pointer_constraints_v1_confine_pointer(constraints_manager,surface,pointer,r,ZWP_POINTER_CONSTRAINTS_V1_LIFETIME_PERSISTENT);zwp_confined_pointer_v1_add_listener(confined,&confined_listener,NULL);wl_region_destroy(r);}return true;}
 if(!strcmp(s,"unconfine")){if(confined){zwp_confined_pointer_v1_destroy(confined);confined=NULL;}return true;}
 return false;
}
