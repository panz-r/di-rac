import { StringArray } from "@shared/proto/dirac/common"
import { OpenAiModelsRequest } from "@shared/proto/dirac/models"
import type { AxiosRequestConfig } from "axios"
import axios from "axios"
import { getAxiosSettings } from "@/shared/net"
import { Logger } from "@/shared/services/Logger"
import { Controller } from ".."

export async function refreshNvidiaNimModels(_controller: Controller, request: OpenAiModelsRequest): Promise<StringArray> {
	try {
		const baseUrl = request.baseUrl || "https://integrate.api.nvidia.com/v1"

		if (!URL.canParse(baseUrl)) {
			return StringArray.create({ values: [] })
		}

		const config: AxiosRequestConfig = {}
		if (request.apiKey) {
			config["headers"] = { Authorization: `Bearer ${request.apiKey}` }
		}

		const response = await axios.get(`${baseUrl}/models`, {
			...config,
			...getAxiosSettings(),
		})
		const modelsArray = response.data?.data?.map((model: any) => model.id) || []
		const models = [...new Set<string>(modelsArray)]

		return StringArray.create({ values: models })
	} catch (error) {
		Logger.error("Error fetching NVIDIA NIM models:", error)
		return StringArray.create({ values: [] })
	}
}
