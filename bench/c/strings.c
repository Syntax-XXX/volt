#include <stdio.h>
#include <string.h>
int main(void){long total=0;char buf[64];for(long i=0;i<500000;i++){snprintf(buf,sizeof buf,"Eintrag %ld von %ld",i,i*2);total+=strlen(buf);}printf("%ld\n",total);return 0;}
