import {describe,it,expect} from 'vitest';
import {daysInView,splitTurn,packSegments,totals,heatLevel} from './calendar';
import type {Turn} from './types';
const t=(start:string,end:string|null,id='a'):Turn=>({id,turn_id:id,session_id:'s',project_id:id,start:Date.parse(start),end:end?Date.parse(end):null,status:end?'completed':'unknown',prompt:'test',tokens:null});
describe('calendar math',()=>{
 it('starts weeks on Monday and lays out months in columns of weeks',()=>{expect(daysInView('2026-11-20','week',1)[0]).toBe('2026-11-16');const d=daysInView('2026-11-20','month',1);expect(d[0]).toBe('2026-10-26');expect(d.length%7).toBe(0);expect(d).toContain('2026-11-30');});
 it('splits across local midnight and retains unknown endings as markers',()=>{const a=splitTurn(t('2026-11-20T14:00:00Z','2026-11-20T17:00:00Z'),'Asia/Tokyo');expect(a.map(x=>x.date)).toEqual(['2026-11-20','2026-11-21']);expect(a[0].end-a[0].start).toBe(3600000);expect(splitTurn(t('2026-11-20T14:00:00Z',null),'Asia/Tokyo')[0].unknown).toBe(true);});
 it('uses separate lanes for overlapping work without hiding turns',()=>{const seg=[...splitTurn(t('2026-11-20T10:00:00Z','2026-11-20T12:00:00Z'),'UTC'),...splitTurn(t('2026-11-20T11:00:00Z','2026-11-20T13:00:00Z','b'),'UTC')];const a=packSegments(seg);expect(a.map(x=>x.lane)).toEqual([0,1]);expect(a.map(x=>x.lanes)).toEqual([2,2]);});
 it('clips totals to selected dates and merges overlap',()=>{const a=[t('2026-11-20T23:00:00Z','2026-11-21T02:00:00Z'),t('2026-11-20T23:30:00Z','2026-11-21T01:00:00Z','b')];const d=totals(a,['2026-11-20'],'UTC');expect(d.total_ms).toBe(5400000);expect(d.elapsed_ms).toBe(3600000);expect(d.count).toBe(2);});
 it('handles DST without inventing missing elapsed time',()=>{const a=t('2026-03-08T05:00:00Z','2026-03-09T04:00:00Z');expect(splitTurn(a,'America/New_York')).toHaveLength(1);expect(totals([a],['2026-03-08'],'America/New_York').total_ms).toBe(23*3600000);});
 it('keeps heat thresholds fixed between months',()=>{expect(heatLevel(0,[1,3,6,9])).toBe(0);expect(heatLevel(2*3600000,[1,3,6,9])).toBe(2);});
});
