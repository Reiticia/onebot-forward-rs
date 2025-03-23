# onebot 协议转发

配置文件示例

```yaml
websocket:
  server:
    host: "127.0.0.1"
    port: 6700
  client:
    host: "127.0.0.1"
    port: 8080
  heartbeat: 30

logger:
  level: info
  file:
    path: "logs/app.log"
    level: warn

blacklist: [123, 456]
whitelist: [789, 101112]

notice:
  smtp: "smtp.example.com"
  username: "user@example.com"
  password: "your_password"
  receiver: "admin@example.com"
  mail:
    subject: "你的Bot掉线了"
    body: "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。"

online_notice: 987654321
```

```json
// app.json 示例
{
  "websocket": {
    "server": {
      "host": "127.0.0.1",
      "port": 6700
    },
    "client": {
      "host": "127.0.0.1",
      "port": 8080
    },
    "heartbeat": 30
  },
  "logger": {
    "level": "info",
    "file": {
      "path": "logs/app.log",
      "level": "warn"
    }
  },
  "blacklist": [123, 456],
  "whitelist": [789, 101112],
  "notice": {
    "smtp": "smtp.example.com",
    "username": "user@example.com",
    "password": "your_password",
    "receiver": "admin@example.com",
    "mail": {
      "subject": "你的Bot掉线了",
      "body": "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。"
    }
  },
  "online_notice": 987654321
}
```

```toml
# app.toml 示例
online_notice = 987654321

blacklist = [123, 456]
whitelist = [789, 101112]

[websocket]
[websocket.server]
host = "127.0.0.1"
port = 6700

[websocket.client]
host = "127.0.0.1"
port = 8080

heartbeat = 30

[logger]
level = "info"

[logger.file]
path = "logs/app.log"
level = "warn"

[notice]
smtp = "smtp.example.com"
username = "user@example.com"
password = "your_password"
receiver = "admin@example.com"

[notice.mail]
subject = "你的Bot掉线了"
body = "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。"
```
