package com.vinaywinai.databeam

import java.io.File

/** Same wrapper rule as desktop; metadata only, no payload copying. */
object PayloadNaming {
    fun root(paths:List<File>,sizeOf:(File)->Long=File::length,stopped:()->Boolean={false}):String {
        require(paths.isNotEmpty()){"Select files or a folder"}
        var largest=paths.first();var largestSize=0L
        for(path in paths){
            check(!stopped()){"Stopped"}
            val size=if(path.isDirectory)path.walkTopDown().sumOf{file->
                check(!stopped()){"Stopped"};if(file.isDirectory)0L else sizeOf(file)
            } else sizeOf(path)
            if(size>largestSize){largest=path;largestSize=size}
        }
        val points=largest.name.ifEmpty{"files"}.codePoints().limit(20).toArray()
        return String(points,0,points.size)+(if(paths.size>1)"_and_more" else "")+"_databeam"
    }
}
