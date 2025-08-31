# onebot 协议转发

## 启动方式

- onebot-forward-rs wss 正向WS服务启动
- onebot-forward-rs wsc 反向WS服务启动（未实现）
- onebot-forward-rs et 邮件发送测试

配置文件示例

* app.yaml/app.yml 示例


```yaml
websocket:
  server:
    host: 0.0.0.0
    port: 6700
    secret: your-secret-key
  client:
    host: 127.0.0.1
    port: 6701
    secret: client-secret
  heartbeat: 30

logger:
  level: info
  exclude:
    - tungstenite::handshake
    - sqlx::query
    - sea_orm_migration::migrator
  file:
    dir: logs
    level: warn

default_policy: deny

notice:
  smtp: smtp.example.com
  username: user@example.com
  password: your-password
  receiver: receiver@example.com
  mail:
    subject: 你的Bot掉线了
    body: "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。"

super_users:
  - 123456
  - 789012

config_db_url: sqlite://forward.sqlite?mode=rwc
convert_self: false
```

* app.json 示例

```json
{
  "websocket": {
    "server": {
      "host": "0.0.0.0",
      "port": 6700,
      "secret": "your-secret-key"
    },
    "client": {
      "host": "127.0.0.1",
      "port": 6701,
      "secret": "client-secret"
    },
    "heartbeat": 30
  },
  "logger": {
    "level": "info",
    "exclude": [
      "tungstenite::handshake",
      "sqlx::query",
      "sea_orm_migration::migrator"
    ],
    "file": {
      "dir": "logs",
      "level": "warn"
    }
  },
  "default_policy": "deny",
  "notice": {
    "smtp": "smtp.example.com",
    "username": "user@example.com",
    "password": "your-password",
    "receiver": "receiver@example.com",
    "mail": {
      "subject": "你的Bot掉线了",
      "body": "OneBot 掉线通知：\\n\\n({bot_id}) 掉线了，请及时处理。"
    }
  },
  "super_users": [123456, 789012],
  "config_db_url": "sqlite://forward.sqlite?mode=rwc",
  "convert_self": false
}
```

* app.toml 示例

```toml
super_users = [123456, 789012]
config_db_url = "sqlite://forward.sqlite?mode=rwc"
default_policy = "deny"
convert_self = false

[websocket]
[websocket.server]
host = "0.0.0.0"
port = 6700
secret = "your-secret-key"

[websocket.client]
host = "127.0.0.1"
port = 6701
secret = "client-secret"
heartbeat = 30

[logger]
level = "info"
exclude = [
    "tungstenite::handshake",
    "sqlx::query",
    "sea_orm_migration::migrator"
]

[logger.file]
dir = "logs"
level = "warn"


[notice]
smtp = "smtp.example.com"
username = "user@example.com"
password = "your-password"
receiver = "receiver@example.com"

[notice.mail]
subject = "你的Bot掉线了"
body = "OneBot 掉线通知：\n\n({bot_id}) 掉线了，请及时处理。"
```
