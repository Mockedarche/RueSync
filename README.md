Early prototyping for a data backup software I'm working on


Current Prototype of a local backup (with 1 second backup interval) fully functional local and lan backups

[Screencast_20260829_110632.webm](https://github.com/user-attachments/assets/17c33798-f54a-403b-aaaf-16e3923ca74f)



Plans for what to work on (roughly in order)

- basic daemon and client foundation - Done
- basic local data backup - Done
- basic lan data backup - Done
- basic wan data backup - Needs testing
- CLI client with full functionality (up to that point) - currently working on
- local host web server - currently working on
- relative paths for backups (you set a folder that all backups are based from)

Things to have fully working before a "release"

- basic functioning data backup (local, lan, and wan)
- Fully functional client CLI
- end to end data encryption for non local data backup
- data compression to reduce data usage
- polish pass

  Note before any release a very major polish pass will be needed. Additionally to realistically release i'd need someone else to be using the software. So this software may stay in a unreleased fashion.

Future plans are ambitious
  
- Drop box behavior
- placeholder files demonstrating a backups contents
- Desktop app
- Mirrored backups (multiple destinations) with torrent style peer file sharing for performance

In short i'd like this software to become techies one stop shop for data backups by having it be feature rich, support extensive configuration, and to also be incredibly user friendly.

web server prototype 
![](https://github.com/Mockedarche/RueSync/blob/main/media/web%20server%20example.png?raw=true)
    
     
  
