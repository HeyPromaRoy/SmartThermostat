# SmartThermostat
Smart thermostat system which is secure, innovative and provides an easy-to-use user experience.
## Installation
Clone the project
```
> git clone https://github.com/hsiaoyin-peng/SmartThermostat.git
```
Set environment variables(create `.env` or copy from project team)  
Store your database key in `.env` file
```
SQLCIPHER_KEY='DATABASE_PASSWORD'
```
Start run the project
```
> cd SmartThermostat
> cargo run
```

## Requirements
1. **Database key :** `.env`
2. **Hash list :** `INTEGIRY.sha256`  
for integrity check to prevent backdoor injection
