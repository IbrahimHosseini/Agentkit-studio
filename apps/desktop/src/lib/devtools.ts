import { request } from './api';
export type Tool = 'browser'|'ios'|'android'|'terminal';
export const names:Record<Tool,string>={browser:'Browser',ios:'iOS Simulator',android:'Android Emulator',terminal:'Terminal'};
export function call<T=any>(conversation_id:string,target:string,action:string,params:Record<string,unknown>={}):Promise<T>{return request(`devtools/${target}/${action}`,{...params,conversation_id});}
export type Frame={data:string;mime_type:string;width:number;height:number;url?:string};
