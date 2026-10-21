#!/usr/bin/env python3
"""Fail early unless D-Bus can resolve this process's Unix identity through NSS."""
import grp
import os
import pwd
import sys


def main():
    uid, gid = os.getuid(), os.getgid()
    if (uid, gid) != (os.geteuid(), os.getegid()):
        raise RuntimeError('CI requires matching real and effective UID/GID')
    try:
        user = pwd.getpwuid(uid)
        group = grp.getgrgid(gid)
        for supplementary in os.getgroups():
            grp.getgrgid(supplementary)
    except KeyError as error:
        raise RuntimeError(
            f'NSS identity missing for runtime UID={uid} GID={gid}; '
            'build the image with CI_UID/CI_GID matching docker run --user'
        ) from error
    if user.pw_gid != gid:
        raise RuntimeError('NSS primary group does not match the runtime GID')
    if pwd.getpwnam(user.pw_name).pw_uid != uid or grp.getgrnam(group.gr_name).gr_gid != gid:
        raise RuntimeError('NSS name/number round trip does not match runtime identity')
    print(f'NSS identity verified: UID={uid} ({user.pw_name}), GID={gid} ({group.gr_name})', flush=True)


if __name__ == '__main__':
    try:
        main()
    except RuntimeError as error:
        sys.exit(str(error))
