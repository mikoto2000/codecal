import {DateTime} from 'luxon';
import type {Turn} from './types';
export type Segment={turn:Turn;date:string;start:number;end:number;top:number;height:number;unknown:boolean;lane:number;lanes:number};
export const localDate=(ms:number,zone:string)=>DateTime.fromMillis(ms,{zone});
export const formatDuration=(ms:number)=>`${Math.floor(ms/3600000)}h ${Math.floor(ms%3600000/60000)}m`;
export const statusLabel={completed:'完了',failed:'失敗',interrupted:'中断',running:'実行中',unknown:'終了不明'};
export function daysInView(anchor:string,view:'week'|'month',weekStart:number):string[]{
 const a=DateTime.fromISO(anchor,{zone:'UTC'});const base=view==='month'?a.startOf('month'):a;
 const first=base.minus({days:(base.weekday%7-weekStart+7)%7});const last=view==='month'?a.endOf('month'):first.plus({days:6});
 const count=view==='month'?Math.ceil((last.startOf('day').diff(first,'days').days+1)/7)*7:7;
 return Array.from({length:count},(_,i)=>first.plus({days:i}).toISODate()!);
}
export function splitTurn(turn:Turn,zone:string):Segment[]{
 let start=turn.start;const end=turn.end??turn.start;const out:Segment[]=[];
 do {
  const day=localDate(start,zone).startOf('day'),next=day.plus({days:1});const stop=Math.min(end,next.toMillis());const length=next.toMillis()-day.toMillis();
  out.push({turn,date:day.toISODate()!,start,end:stop,top:(start-day.toMillis())/length*1440,height:Math.max(turn.end===null?22:2,(stop-start)/length*1440),unknown:turn.end===null,lane:0,lanes:1});
  start=stop;
 }while(start<end);
 return out;
}
export function packSegments(segments:Segment[]):Segment[]{
 const sorted=segments.map(s=>({...s})).sort((a,b)=>a.top-b.top||b.height-a.height);let group:Segment[]=[];let ends:number[]=[];let boundary=-1;
 const finish=()=>{for(const s of group)s.lanes=ends.length;group=[];ends=[];boundary=-1;};
 for(const s of sorted){if(s.top>=boundary)finish();let lane=ends.findIndex(e=>e<=s.top);if(lane<0)lane=ends.length;s.lane=lane;ends[lane]=s.top+s.height;boundary=Math.max(boundary,s.top+s.height);group.push(s);}finish();return sorted;
}
export function totals(turns:Turn[],days:string[],zone:string){
 const dates=new Set(days);const ranges:[number,number][]=[];const projects:Record<string,number>={};const ids=new Set<string>();let total_ms=0;
 for(const t of turns)for(const s of splitTurn(t,zone)){if(!dates.has(s.date))continue;ids.add(t.id);if(t.end===null)continue;const duration=s.end-s.start;total_ms+=duration;projects[t.project_id]=(projects[t.project_id]??0)+duration;ranges.push([s.start,s.end]);}
 ranges.sort((a,b)=>a[0]-b[0]);let elapsed_ms=0,left=0,right=0;
 for(const [s,e] of ranges){if(s>right){elapsed_ms+=right-left;left=s;right=e;}else right=Math.max(right,e);}elapsed_ms+=right-left;
 return {elapsed_ms,total_ms,count:ids.size,projects};
}
export function heatLevel(ms:number,thresholds:number[]){if(ms<=0)return 0;return Math.min(4,1+thresholds.filter(h=>ms>=h*3600000).length);}
