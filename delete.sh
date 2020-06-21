#!/bin/sh

id=$1

curl 'https://api.fanbox.cc/post.delete' \
	-H 'Content-Type: application/json' \
	-H "Origin: https://$CREATOR_ID.fanbox.cc" \
	-H "X-CSRF-Token: $CSRF_TOKEN" \
	-H "Cookie: FANBOXSESSID=$SESSID" \
	--data-raw '{"postId":"'$id'"}'
