#include <stdio.h>
#include <stdlib.h>
int main(void){int n=5000000;char*f=malloc(n+1);for(int i=0;i<=n;i++)f[i]=1;for(long i=2;i*i<=n;i++)if(f[i])for(long j=i*i;j<=n;j+=i)f[j]=0;long c=0;for(int k=2;k<=n;k++)if(f[k])c++;printf("%ld\n",c);return 0;}
