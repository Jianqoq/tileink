// Count and emit share one DDA traversal. Tile-top tests, endpoint snapping and
// clip nudges have distinct tolerances; combining them changes winding coverage.
struct Traversal {
    uint4 bounds;
    uint backdrop;
    float4 points;
    bool down;
    uint count,begin,end;
    float a,b,direction,x0,y0;
    int row_begin,row_end;
};
bool top_boundary(float y) {return abs(y-floor(y))<=1.0e-12f;}
int boundary_ceil(float y) {return int(top_boundary(y)?floor(y):ceil(y));}
uint tile_span(float a,float b) {return uint(max(ceil(max(a,b))-floor(min(a,b)),1.0f));}
float4 read_float4(const device uint* words,uint offset) {return as_type<float4>(uint4(words[offset],words[offset+1],words[offset+2],words[offset+3]));}

bool traversal(const device uint* lines,const device uint* paths,uint path_count,uint index,thread Traversal& scan) {
    uint line=index*6,path_id=lines[line];
    // Test the record index before multiplication or any native pointer access.
    // Missing path records have empty geometry on every backend.
    if(path_id>=path_count) return false;
    uint path=path_id*19;
    scan.bounds=uint4(paths[path+6],paths[path+7],paths[path+8],paths[path+9]);
    scan.backdrop=paths[path+4];
    if(scan.bounds.x==scan.bounds.z || scan.bounds.y>=scan.bounds.w) return false;
    float4 transform=read_float4(paths,path+13);
    float2 translation=as_type<float2>(uint2(paths[path+17],paths[path+18]));
    float4 local=read_float4(lines,line+2);
    float2 first=float2((transform.x*local.x+transform.z*local.y)+translation.x,(transform.y*local.x+transform.w*local.y)+translation.y);
    float2 last=float2((transform.x*local.z+transform.z*local.w)+translation.x,(transform.y*local.z+transform.w*local.w)+translation.y);
    scan.down=last.y>=first.y;
    scan.points=scan.down?float4(first,last):float4(last,first);
    float4 scaled=scan.points*(1.0f/16.0f);
    uint columns=tile_span(scaled.x,scaled.z)-1;
    scan.count=columns+tile_span(scaled.y,scaled.w);
    float dx=abs(scaled.z-scaled.x),dy=scaled.w-scaled.y;
    if(dx+dy==0 || (dy==0 && floor(scaled.y)==scaled.y)) return false;
    float inverse=1.0f/(dx+dy);
    scan.a=dx*inverse;
    bool positive=scaled.z>=scaled.x;
    scan.direction=positive?1.0f:-1.0f;
    float left=floor(scaled.x*scan.direction);
    float fraction=scaled.x*scan.direction-left;
    scan.y0=floor(scaled.y);
    float next_y=scaled.y==scaled.w?ceil(scaled.y):scan.y0+1.0f;
    scan.b=min((dy*fraction+dx*(next_y-scaled.y))*inverse,0.99999994f);
    float error=floor(scan.a*(float(scan.count)-1.0f)+scan.b)-float(columns);
    if(error!=0) scan.a+=error>0?-0.0000002f:0.0000002f;
    scan.x0=left*scan.direction-(positive?0.0f:1.0f);
    float minimum_x=min(scaled.x,scaled.z),maximum_x=max(scaled.x,scaled.z);
    if(scaled.y>=float(scan.bounds.w) || scaled.w<=float(scan.bounds.y)+1.0e-12f || minimum_x>=float(scan.bounds.z)) return false;
    scan.begin=0;scan.end=scan.count;
    if(scaled.y<float(scan.bounds.y)) {
        float cut=rint((float(scan.bounds.y)-scan.y0+scan.b-scan.a)/(1.0f-scan.a))-1.0f;
        if(scan.y0+cut-floor(scan.a*cut+scan.b)<float(scan.bounds.y)) cut+=1;
        scan.begin=uint(cut);
    }
    if(scaled.w>float(scan.bounds.w)) {
        float cut=rint((float(scan.bounds.w)-scan.y0+scan.b-scan.a)/(1.0f-scan.a))-1.0f;
        if(scan.y0+cut-floor(scan.a*cut+scan.b)<float(scan.bounds.w)) cut+=1;
        scan.end=uint(cut);
    }
    scan.row_begin=0;scan.row_end=0;
    if(maximum_x<float(scan.bounds.x)) {
        scan.row_begin=boundary_ceil(scaled.y);scan.row_end=boundary_ceil(scaled.w);scan.end=scan.begin;
    } else {
        float adjustment=positive?0.0f:1.0f;
        if(minimum_x<float(scan.bounds.x)) {
            float cut=rint((scan.direction*(float(scan.bounds.x)-scan.x0)-scan.b+adjustment)/scan.a);
            if((scan.x0+scan.direction*floor(scan.a*cut+scan.b)<float(scan.bounds.x))==positive) cut+=1;
            int next_row=int(scan.y0+cut-floor(scan.a*cut+scan.b)+1.0f);
            if(positive && uint(cut)>scan.begin) {
                scan.row_begin=int(top_boundary(scaled.y)?scan.y0:scan.y0+1.0f);
                scan.row_end=next_row;scan.begin=uint(cut);
            } else if(!positive && uint(cut)<scan.end) {
                scan.row_begin=next_row;scan.row_end=boundary_ceil(scaled.w);scan.end=uint(cut);
            }
        }
        if(maximum_x>float(scan.bounds.z)) {
            float cut=rint((scan.direction*(float(scan.bounds.z)-scan.x0)-scan.b+adjustment)/scan.a);
            if((scan.x0+scan.direction*floor(scan.a*cut+scan.b)<float(scan.bounds.z))==positive) cut+=1;
            if(positive) scan.end=min(scan.end,uint(cut));else scan.begin=max(scan.begin,uint(cut));
        }
    }
    scan.end=max(scan.begin,scan.end);
    scan.row_begin=max(scan.row_begin,int(scan.bounds.y));scan.row_end=min(scan.row_end,int(scan.bounds.w));
    return true;
}
int2 traversal_tile(Traversal scan,uint index,float z) {return int2(scan.x0+scan.direction*z,scan.y0+float(index)-z);}
bool traversal_contains(Traversal scan,int2 tile) {return all(tile>=int2(scan.bounds.xy)) && all(tile<int2(scan.bounds.zw));}
uint traversal_offset(Traversal scan,int2 tile) {return scan.backdrop+(uint(tile.y)-scan.bounds.y)*(scan.bounds.z-scan.bounds.x)+uint(tile.x)-scan.bounds.x;}
