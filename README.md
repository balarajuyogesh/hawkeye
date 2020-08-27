# video-slate-detector
Detect slate in a RTP stream and hit callback

## Running with Docker

```bash
docker run -p 5000:5000/udp -v /home/user/images/:/local -it video-slate-detector:0.0.1 /local/slate_120px.jpg http://localhost:8000
```
