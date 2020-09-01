# Hawkeye
Detect images in a video stream and execute automated actions.

## Running with Docker

```bash
docker build -t video-slate-detector:0.0.1 .
docker run -p 5000:5000/udp -v /home/user/images/:/local -it video-slate-detector:0.0.1 /local/slate_120px.jpg http://localhost:8000
```
