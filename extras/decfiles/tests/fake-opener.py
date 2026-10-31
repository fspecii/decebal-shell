#!/usr/bin/env python3
"""Registered fake opener for the private Open fixtures: fake-opener.py LOGDIR NAME ARGS...
Records its exact argv bytes (hex) and never interprets, opens or executes any argument."""
import json,os,sys,time
log,name,args=sys.argv[1],sys.argv[2],sys.argv[3:]
record={'opener':name,'argv_hex':[os.fsencode(a).hex() for a in args],'pid':os.getpid(),'cwd':os.getcwd()}
tmp=os.path.join(log,'.tmp-%d'%os.getpid())
with open(tmp,'w') as f:json.dump(record,f)
os.rename(tmp,os.path.join(log,'%d-%s-%d.json'%(time.time_ns(),name,os.getpid())))
